#![no_std]
#![no_main]

//! CSI LiteTUI — a handheld Wi-Fi CSI scope for the M5Stack CoreS3 SE.
//!
//! Core 0 (this `main`, async on esp-rtos + embassy) runs the Wi-Fi / CSI
//! pipeline. Core 1 (started via [`CpuControl::start_app_core`]) owns the LCD,
//! touch panel and SD card and runs the scientific UI. The two cores share
//! state through [`shared`] — live atomics for the view and a lock-free byte
//! ring for serialized log records.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::ToString;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU16, Ordering};

use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_time::{Duration, Instant, Timer};

use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::system::{CpuControl, Stack};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_hal_bus::spi::RefCellDevice;
use display_interface_spi::SPIInterface;
use mipidsi::options::{ColorInversion, ColorOrder};
use mipidsi::{models::ILI9342CRgb565, Builder};
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig};
use ratatui::Terminal;

use embedded_sdmmc::{SdCard, VolumeManager};

use esp_csi_rs::logging::logging::{init_logger, LogMode};
use esp_csi_rs::{
    CSINode, CSINodeClient, CollectorMode, EmitterConfig, NodeHardware, NodeRole, WifiApConfig,
    WifiSnifferConfig, WifiStationConfig,
};
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethod, Protocol, WifiController};

use {esp_backtrace as _, esp_println as _};

mod board;
mod capture;
mod config;
mod logging;
mod shared;
mod ui;

use config::{Config, LogFormat, NodeMode};
use ui::{Action, Ui};

static WIFI_CONTROLLER: static_cell::StaticCell<WifiController<'static>> =
    static_cell::StaticCell::new();
static mut CORE1_STACK: Stack<65536> = Stack::new();
static FILE_COUNTER: AtomicU16 = AtomicU16::new(1);

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // Bring up the logger for our own `log_ln!`/`println!` lines, but suppress
    // the per-packet UART CSI dump — CSI goes to the LCD and SD card.
    init_logger(spawner, LogMode::Text);
    esp_csi_rs::set_csi_logging_enabled(false);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 61440);
    // Add PSRAM as an external heap region. The on-board LCD font tables
    // (mousefood / embedded-graphics-unicodefonts) are large; without this the
    // ~60 KB internal heap is exhausted during UI init on core 1.
    esp_alloc::psram_allocator!(p.PSRAM, esp_hal::psram);

    let timg0 = TimerGroup::new(p.TIMG0);
    let sw_int = SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // Radio + Wi-Fi interfaces.
    let radio_cfg = esp_radio::wifi::ControllerConfig::default();
    let (wifi_controller, mut interfaces) =
        esp_radio::wifi::new(p.WIFI, radio_cfg).expect("Wi-Fi init failed");
    let controller = WIFI_CONTROLLER.init(wifi_controller);

    // --- Spawn the UI / SD / touch engine on core 1 ---
    let mut cpu_control = CpuControl::new(p.CPU_CTRL);
    let core1_stack = unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) };

    let i2c0 = p.I2C0;
    let sda = p.GPIO12;
    let scl = p.GPIO11;
    let spi3 = p.SPI3;
    let sck = p.GPIO36;
    let mosi = p.GPIO37;
    let miso = p.GPIO35;
    let lcd_cs = p.GPIO3;
    let sd_cs = p.GPIO4;

    let _core1_guard = cpu_control
        .start_app_core(core1_stack, move || {
            core1_main(i2c0, sda, scl, spi3, sck, mosi, miso, lcd_cs, sd_cs);
        })
        .expect("failed to start core 1");

    // --- Core 0: capture loop ---
    // The node owns the Wi-Fi hardware, so build it once and then reconfigure +
    // re-run it for each capture. esp-csi-rs resets its own globals when `run()`
    // returns, so a node is reusable — the user never has to reset the device
    // between captures.
    wait_until_running().await;

    let csi_hardware = NodeHardware::new(&mut interfaces, controller);
    let mut node = CSINode::new(
        node_role(&Config::load()),
        Some(Config::load().csi_config()),
        Some(Config::load().traffic_hz),
        csi_hardware,
    );

    loop {
        let cfg = Config::load();
        node.set_role(node_role(&cfg));
        // An emitter captures nothing, so there is no CSI to deliver.
        node.set_csi_output_enabled(cfg.mode.captures_csi());
        // The node is reused across captures and `io_tasks` persists, so a
        // TX-only emitter run must not leave RX disabled for the next mode.
        node.set_io_tasks(cfg.mode.io_tasks());
        node.set_csi_config(cfg.csi_config());
        node.set_traffic_frequency(cfg.traffic_hz);
        // Every mode here runs on N: HT40 capture needs an HT protocol, and an
        // emitter pins its own HT set during bring-up (this is ignored there).
        node.set_protocol(Protocol::N);

        // Select the CSI delivery path. Re-applied each run because `run()`'s
        // teardown resets the delivery gates back to Off.
        match cfg.delivery {
            config::DeliveryMode::Callback => {
                // Opens the publish gate and selects Callback mode.
                esp_csi_rs::set_csi_callback(capture::on_csi_callback);
            }
            config::DeliveryMode::Async => {
                // Leave the mode at Off here: the drain task's first
                // `next_csi_packet().await` lazily flips Off -> Async *and*
                // opens the CSI publish gate. Pre-setting Async would skip that
                // gate-open, so the Wi-Fi callback would build/emit no packets.
                // `clear_csi_callback` guarantees the mode is Off going in.
                esp_csi_rs::clear_csi_callback();
            }
        }

        let mut drain_client = CSINodeClient::new();
        let stop_client = CSINodeClient::new();
        let stopper = async {
            while !shared::stop_requested() {
                Timer::after(Duration::from_millis(50)).await;
            }
            stop_client.send_stop().await;
        };

        join3(node.run(), stopper, capture::run_drain(&mut drain_client)).await;

        // Capture done. Wait for the UI to configure and start the next one.
        wait_until_running().await;
    }
}

/// Block (asynchronously) until the UI moves the run state to "running".
async fn wait_until_running() {
    while shared::run_state() != shared::RUN_RUNNING {
        Timer::after(Duration::from_millis(50)).await;
    }
}

/// Build the esp-csi-rs node role for the selected mode.
fn node_role(cfg: &Config) -> NodeRole {
    match cfg.mode {
        NodeMode::Station => NodeRole::Collector(CollectorMode::Station(WifiStationConfig::new(
            StationConfig::default()
                .with_ssid(config::WIFI_SSID)
                .with_password(config::WIFI_PASSWORD.to_string())
                .with_auth_method(AuthenticationMethod::Wpa2Personal),
        ))),
        NodeMode::Sniffer => NodeRole::Collector(CollectorMode::Sniffer(
            WifiSnifferConfig::default().with_channel(cfg.channel),
        )),
        NodeMode::AccessPoint => NodeRole::Collector(CollectorMode::AccessPoint(
            WifiApConfig::new(
                AccessPointConfig::default()
                    .with_ssid(config::AP_SSID)
                    .with_channel(cfg.channel),
                cfg.channel,
                cfg.ht40,
            ),
        )),
        // Both emitter modes share one config; only the bandwidth differs.
        NodeMode::Ht20Emitter | NodeMode::Ht40Emitter => NodeRole::Emitter(emitter_cfg(cfg)),
    }
}

/// Emitter config for the selected channel / bandwidth / injection rate.
///
/// The traffic-frequency field doubles as the emitter's injection rate, so the
/// setup screen's `Traffic` steps map straight onto the inject period.
fn emitter_cfg(cfg: &Config) -> EmitterConfig {
    EmitterConfig::new(cfg.channel, cfg.mode.emitter_bandwidth(cfg.ht40))
        .with_period(Duration::from_hz(cfg.traffic_hz.max(1) as u64))
}

/// Core-1 entry point: hardware bring-up + the blocking UI / SD loop.
#[allow(clippy::too_many_arguments)]
fn core1_main(
    i2c0: esp_hal::peripherals::I2C0<'static>,
    sda: esp_hal::peripherals::GPIO12<'static>,
    scl: esp_hal::peripherals::GPIO11<'static>,
    spi3: esp_hal::peripherals::SPI3<'static>,
    sck: esp_hal::peripherals::GPIO36<'static>,
    mosi: esp_hal::peripherals::GPIO37<'static>,
    miso: esp_hal::peripherals::GPIO35<'static>,
    lcd_cs: esp_hal::peripherals::GPIO3<'static>,
    sd_cs: esp_hal::peripherals::GPIO4<'static>,
) -> ! {
    let mut i2c = I2c::new(i2c0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .unwrap()
        .with_sda(sda)
        .with_scl(scl);

    board::init_power_management(&mut i2c);
    let delay = Delay::new();
    delay.delay_millis(100);

    let spi = Spi::new(
        spi3,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(20))
            .with_mode(SpiMode::_0),
    )
    .unwrap()
    .with_sck(sck)
    .with_mosi(mosi)
    .with_miso(miso);

    // The LCD and SD card share the bus; arbitrate with a RefCell.
    let spi_bus: &'static RefCell<_> = Box::leak(Box::new(RefCell::new(spi)));

    let lcd_cs = Output::new(lcd_cs, Level::High, OutputConfig::default());
    let sd_cs = Output::new(sd_cs, Level::High, OutputConfig::default());

    let lcd_spi = RefCellDevice::new_no_delay(spi_bus, lcd_cs).unwrap();
    let sd_spi = board::SdSpiDevice {
        inner: RefCellDevice::new_no_delay(spi_bus, sd_cs).unwrap(),
    };

    let di = SPIInterface::new(lcd_spi, board::LcdDcPin);
    let mut display = Builder::new(ILI9342CRgb565, di)
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Delay::new())
        .expect("LCD init failed");

    Rectangle::new(Point::new(0, 0), Size::new(320, 240))
        .into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::BLACK).build())
        .draw(&mut display)
        .ok();

    let backend = EmbeddedBackend::new(&mut display, EmbeddedBackendConfig::default());
    let mut terminal = Terminal::new(backend).expect("ratatui init failed");

    let sd_card = SdCard::new(sd_spi, Delay::new());
    let mut sink = logging::SdSink::new(VolumeManager::new(sd_card, board::BuildClock));

    let mut ui = Ui::new(Instant::now().as_millis());
    let mut finger_down = false;
    let mut next_touch_ms: u64 = 0;
    let mut last_draw_ms: u64 = 0;
    let mut last_flush_ms: u64 = 0;

    esp_println::println!(">>> core 1: UI / touch / SD engine online");

    loop {
        let now = Instant::now().as_millis();
        poll_touch(&mut i2c, &mut ui, &mut sink, &mut finger_down, &mut next_touch_ms, now);

        ui.tick(now);

        // While capturing, keep the SD file fed from the cross-core ring with a
        // bounded write each frame, and flush on a slow cadence (flushing is the
        // expensive part). This keeps the LCD/touch responsive during logging.
        if sink.is_logging() && shared::run_state() == shared::RUN_RUNNING {
            sink.drain();
            if now.saturating_sub(last_flush_ms) >= 1000 {
                sink.flush();
                last_flush_ms = now;
            }
        }

        // Throttle the heavy LCD redraw; stamp the time *after* it so a slow
        // draw still leaves a full interval for touch polling before the next.
        if now.saturating_sub(last_draw_ms) >= DRAW_INTERVAL_MS {
            let _ = terminal.draw(|f| ui.render(f));
            // The redraw blocks; re-poll immediately so a tap held during it
            // isn't missed while we were busy pushing pixels.
            let after = Instant::now().as_millis();
            poll_touch(&mut i2c, &mut ui, &mut sink, &mut finger_down, &mut next_touch_ms, after);
            last_draw_ms = after;
        }

        delay.delay_millis(POLL_MS);
    }
}

// Poll touch on a tight cadence; throttle the (blocking) LCD redraw so the loop
// spends most of its time sampling input instead of pushing pixels.
const POLL_MS: u32 = 8;
const TOUCH_DEBOUNCE_MS: u64 = 150;
const DRAW_INTERVAL_MS: u64 = 80;

/// Read the touch panel once and dispatch an edge-triggered, debounced tap.
fn poll_touch<I, D, T>(
    i2c: &mut I,
    ui: &mut Ui,
    sink: &mut logging::SdSink<D, T>,
    finger_down: &mut bool,
    next_touch_ms: &mut u64,
    now: u64,
) where
    I: embedded_hal::i2c::I2c,
    D: embedded_sdmmc::BlockDevice,
    T: embedded_sdmmc::TimeSource,
    D::Error: core::fmt::Debug,
{
    match board::read_touch(i2c) {
        Some((x, y)) => {
            if !*finger_down && now >= *next_touch_ms {
                *finger_down = true;
                *next_touch_ms = now + TOUCH_DEBOUNCE_MS;
                handle_action(ui.on_touch(x, y, now), sink);
            }
        }
        None => *finger_down = false,
    }
}

fn handle_action<D, T>(action: Action, sink: &mut logging::SdSink<D, T>)
where
    D: embedded_sdmmc::BlockDevice,
    T: embedded_sdmmc::TimeSource,
    D::Error: core::fmt::Debug,
{
    match action {
        Action::StartCapture => {
            shared::RING_DROPS.store(0, Ordering::Relaxed);
            shared::SD_RECORDS.store(0, Ordering::Relaxed);
            let fmt = LogFormat::from_u8(shared::LOG_FORMAT.load(Ordering::Relaxed));
            let id = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            sink.start(fmt, id);
            shared::start_capture(shared::MODE.load(Ordering::Relaxed));
        }
        Action::StopCapture => {
            // Stop the pipeline and flush what's buffered, but keep the file
            // open so the save prompt can still save or discard it.
            shared::request_stop();
            sink.drain();
        }
        Action::Save => {
            sink.finish(true);
            shared::RUN.store(shared::RUN_SAVED, Ordering::Release);
        }
        Action::Discard => {
            sink.finish(false);
            shared::RUN.store(shared::RUN_DISCARDED, Ordering::Release);
        }
        Action::None => {}
    }
}
