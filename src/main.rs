#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::ToString;
use core::sync::atomic::{Ordering, AtomicU16};

use embassy_executor::Spawner;
use embassy_futures::join::join; 
use embassy_time::{Duration, Timer, Instant}; // Added Instant for timestamps

use esp_csi_rs::logging::logging::{init_logger, LogMode};
use esp_csi_rs::{
    config::CsiConfig, CSINode, CollectionMode, Node, CentralOpMode,
    PeripheralOpMode, WifiSnifferConfig, EspNowConfig
};
use esp_csi_rs::{CSINodeClient, CSINodeHardware, WifiStationConfig};

// --- HARDWARE IMPORTS ---
use esp_hal::{
    system::{CpuControl, Stack},
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    spi::{master::{Config as SpiConfig, Spi}, Mode as SpiMode},
    time::Rate,
    clock::CpuClock,
    timer::timg::TimerGroup,
};

// --- SHARED BUS & SD CARD IMPORTS ---
use core::cell::RefCell;
use embedded_hal_bus::spi::RefCellDevice;
use embedded_sdmmc::{SdCard, VolumeManager, TimeSource, Timestamp, Mode, VolumeIdx};

// --- DISPLAY IMPORTS ---
use display_interface_spi::SPIInterface;
use mipidsi::{models::ILI9342CRgb565, Builder, options::{ColorOrder, ColorInversion}};
use embedded_graphics::{
    prelude::*,
    primitives::{Rectangle, PrimitiveStyleBuilder},
    pixelcolor::Rgb565,
};

// --- RATATUI IMPORTS ---
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig};
use ratatui::Terminal;

use esp_radio::{
    wifi::{ClientConfig, WifiController},
    Controller,
};
use {esp_backtrace as _, esp_println as _};

mod csi;
mod ui;

use csi::{node_task, SELECTED_MODE, RECORDING_STATE};
use ui::{render_frame, Page};

// ==========================================
// GLOBALS & DUMMY RTC
// ==========================================
static WIFI_CONTROLLER: static_cell::StaticCell<WifiController<'static>> = static_cell::StaticCell::new();
static mut CORE1_STACK: Stack<65536> = Stack::new();
esp_bootloader_esp_idf::esp_app_desc!();

struct DummyClock;
impl TimeSource for DummyClock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp { year_since_1970: 56, zero_indexed_month: 3, zero_indexed_day: 15, hours: 12, minutes: 0, seconds: 0 }
    }
}

static FILE_COUNTER: AtomicU16 = AtomicU16::new(1);

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

// ==========================================
// HARDWARE ARBITRATION WRAPPERS (CoreS3 GPIO35 Fix)
// ==========================================
use embedded_hal::digital::{ErrorType as DigitalErrorType, OutputPin};
use embedded_hal::spi::{ErrorType as SpiErrorType, Operation, SpiDevice};
use core::convert::Infallible;

struct LcdDcPin;

impl DigitalErrorType for LcdDcPin {
    type Error = Infallible;
}

impl OutputPin for LcdDcPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        unsafe {
            core::ptr::write_volatile((0x6000_4000 + 0x0018) as *mut u32, 1 << 3);
            core::ptr::write_volatile((0x6000_4000 + 0x0030) as *mut u32, 1 << 3);
        }
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        unsafe {
            core::ptr::write_volatile((0x6000_4000 + 0x0014) as *mut u32, 1 << 3);
            core::ptr::write_volatile((0x6000_4000 + 0x0030) as *mut u32, 1 << 3);
        }
        Ok(())
    }
}

struct SdSpiDevice<T> {
    inner: T,
}

impl<T: SpiErrorType> SpiErrorType for SdSpiDevice<T> {
    type Error = T::Error;
}

impl<T: SpiDevice> SpiDevice for SdSpiDevice<T> {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        unsafe {
            core::ptr::write_volatile((0x6000_4000 + 0x0034) as *mut u32, 1 << 3);
        }
        self.inner.transaction(operations)
    }
}

// ==========================================
// MAIN
// ==========================================
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    
    init_logger(spawner, LogMode::Text);
    
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 61440);
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let radio_init = mk_static!(Controller<'static>, esp_radio::init().expect("Failed to init Wi-Fi"));
    let mut config_radio = esp_radio::wifi::Config::default();
    config_radio = config_radio.with_power_save_mode(esp_radio::wifi::PowerSaveMode::None);
    let (wifi_controller, mut interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, config_radio).unwrap();
    let controller = WIFI_CONTROLLER.init(wifi_controller);

    let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);
    let core1_stack_ptr = unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) };

    let i2c0 = peripherals.I2C0;
    let sda = peripherals.GPIO12;
    let scl = peripherals.GPIO11;
    let spi3 = peripherals.SPI3;
    let sck = peripherals.GPIO36;
    let mosi = peripherals.GPIO37;
    
    let lcd_cs_pin = peripherals.GPIO3;
    let sd_miso_pin = peripherals.GPIO35; 

    let _core1_guard = cpu_control.start_app_core(core1_stack_ptr, move || {
        let mut i2c = I2c::new(i2c0, I2cConfig::default().with_frequency(Rate::from_khz(400))).unwrap().with_sda(sda).with_scl(scl);

        let _ = i2c.write(0x34, &[0x16, 0x07]); 
        let _ = i2c.write(0x34, &[0x90, 0xBF]); 
        let _ = i2c.write(0x34, &[0x93, 0x1C]); 
        let _ = i2c.write(0x34, &[0x95, 0x1C]); 
        let _ = i2c.write(0x34, &[0x99, 0x1C]); 

        let delay = Delay::new();
        delay.delay_millis(100);
        
        let mut reg_val = [0u8; 1];
        let _ = i2c.write_read(0x58, &[0x05], &mut reg_val);
        let _ = i2c.write(0x58, &[0x05, reg_val[0] & !0x02]);
        let _ = i2c.write(0x58, &[0x03, reg_val[0] | 0x02]);

        let mut config_p0 = [0u8; 1];
        let _ = i2c.write_read(0x58, &[0x04], &mut config_p0);
        let _ = i2c.write(0x58, &[0x04, config_p0[0] & !0x01]); 

        let mut out_p0 = [0u8; 1];
        let _ = i2c.write_read(0x58, &[0x02], &mut out_p0);
        let _ = i2c.write(0x58, &[0x02, out_p0[0] | 0x01]); 

        let spi = Spi::new(spi3, SpiConfig::default().with_frequency(Rate::from_mhz(20)).with_mode(SpiMode::_0))
            .unwrap().with_sck(sck).with_mosi(mosi).with_miso(sd_miso_pin);
        
        let spi_bus: &'static RefCell<_> = alloc::boxed::Box::leak(alloc::boxed::Box::new(RefCell::new(spi)));

        let lcd_cs = Output::new(lcd_cs_pin, Level::High, OutputConfig::default());
        let sd_cs = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());

        let lcd_spi_device = RefCellDevice::new_no_delay(spi_bus, lcd_cs).unwrap();
        let sd_spi_device = SdSpiDevice { 
            inner: RefCellDevice::new_no_delay(spi_bus, sd_cs).unwrap() 
        };

        let dc = LcdDcPin; 
        let di = SPIInterface::new(lcd_spi_device, dc);

        let mut display = Builder::new(ILI9342CRgb565, di)
            .color_order(ColorOrder::Bgr)
            .invert_colors(ColorInversion::Inverted)
            .init(&mut Delay::new()).expect("LCD Failed to Init");

        Rectangle::new(Point::new(0, 0), Size::new(320, 240)).into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::BLACK).build()).draw(&mut display).unwrap();

        let backend = EmbeddedBackend::new(&mut display, EmbeddedBackendConfig::default());
        let mut terminal = Terminal::new(backend).expect("Failed to initialize Ratatui");

        let block_device = SdCard::new(sd_spi_device, Delay::new());
        let volume_mgr = VolumeManager::new(block_device, DummyClock);
        
        let mut sd_active = false;
        let mut filename_str = alloc::string::String::new();

        let delay = Delay::new();
        
        // Removed csi_amps_f64 -> Replaced with packet ring buffer
        let mut csi_phases_f64 = [(0.0f64, 0.0f64); 64];
        let mut waterfall_history = [[0u8; 64]; 30];
        
        // --- NEW DATA STRUCTURES FOR ADVANCED TABS ---
        let mut packet_ring = [([0u8; 64], 0u64); 20];
        let mut ring_idx = 0;
        let mut last_pkt_count = 0;

        let mut rssi_history_points = [(0.0f64, 0.0f64); 100];
        let mut ema_points = [(0.0f64, 0.0f64); 100];
        let mut rssi_count = 0;
        let alpha = 0.2_f64;
        let mut current_ema: Option<f64> = None;
        let start_time_ms = Instant::now().as_millis();

        let mut touch_cooldown = 0;
        let mut finger_down = false;
        let mut current_page = Page::Welcome;
        let mut active_tab = 0; 

        esp_println::println!(">>> CORE 1: UI AND TOUCH ENGINE ONLINE <<<");

        loop {
            if touch_cooldown > 0 { touch_cooldown -= 1; }

            let mut touch_data = [0u8; 5];
            if i2c.write_read(0x38, &[0x02], &mut touch_data).is_ok() {
                let touch_count = touch_data[0] & 0x0F; 
                if touch_count > 0 {
                    if !finger_down && touch_cooldown == 0 {
                        let raw_x = (((touch_data[1] & 0x0F) as u16) << 8) | touch_data[2] as u16;
                        let raw_y = (((touch_data[3] & 0x0F) as u16) << 8) | touch_data[4] as u16;
                        finger_down = true;
                        
                        touch_cooldown = 15; 

                        match current_page {
                            Page::Welcome => {
                                if raw_x > 20 && raw_x < 300 && raw_y > 60 && raw_y < 220 { current_page = Page::Settings; }
                            }
                            Page::Settings => {
                                if raw_y < 80 { SELECTED_MODE.store(1, Ordering::Relaxed); } 
                                else if raw_y > 80 && raw_y < 160 { SELECTED_MODE.store(2, Ordering::Relaxed); } 
                                else if raw_y > 160 { SELECTED_MODE.store(3, Ordering::Relaxed); }
                                
                                let file_id = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
                                filename_str = alloc::format!("C_{:04}.CSV", file_id);

                                if let Ok(volume) = volume_mgr.open_volume(VolumeIdx(0)) {
                                    if let Ok(rd) = volume.open_root_dir() {
                                        if let Ok(f) = rd.open_file_in_dir(filename_str.as_str(), Mode::ReadWriteCreateOrTruncate) {
                                            let header = "Timestamp_Host,MAC,RSSI,Rate,Noise,Channel,CSILength,Raw_CSI\n";
                                            let _ = f.write(header.as_bytes());
                                            let _ = f.close(); 
                                            sd_active = true;
                                            esp_println::println!(">>> SD CARD: Logging to {}", filename_str);
                                        }
                                    }
                                } else {
                                    esp_println::println!(">>> SD CARD: Warning - No card detected!");
                                }
                                current_page = Page::Radar;
                            }
                            Page::Radar => {
                                if raw_y < 50 {
                                    if raw_x < 80 { 
                                        RECORDING_STATE.store(1, Ordering::Relaxed);
                                        current_page = Page::SavePrompt;
                                    } else if raw_x > 240 {
                                        // UPDATED: Now cycles through 5 tabs
                                        active_tab = (active_tab + 1) % 6;
                                    }
                                }
                            }
                            Page::SavePrompt => {
                                if raw_y > 60 && raw_y < 160 {
                                    if raw_x > 40 && raw_x < 150 { 
                                        sd_active = false;
                                        esp_println::println!(">>> SD CARD: Saved.");
                                        RECORDING_STATE.store(2, Ordering::Relaxed);
                                        current_page = Page::Halted;
                                    } else if raw_x > 170 && raw_x < 280 { 
                                        sd_active = false;
                                        if let Ok(volume) = volume_mgr.open_volume(VolumeIdx(0)) {
                                            if let Ok(rd) = volume.open_root_dir() {
                                                let _ = rd.delete_file_in_dir(filename_str.as_str());
                                                esp_println::println!(">>> SD CARD: Deleted.");
                                            }
                                        }
                                        RECORDING_STATE.store(3, Ordering::Relaxed);
                                        current_page = Page::Halted;
                                    }
                                }
                            }
                            Page::Halted => {}
                        }
                    }
                } else {
                    finger_down = false;
                }
            }

            let now_ms = Instant::now().as_millis();
            let packet_count = csi::PACKET_COUNT.load(Ordering::Relaxed);
            let rssi = csi::CURRENT_RSSI.load(Ordering::Relaxed);

            if current_page == Page::Radar {
                
                // TRACK ADVANCED UI METRICS WHEN PACKETS ARRIVE
                if packet_count != last_pkt_count {
                    last_pkt_count = packet_count;

                    // 1. Snapshot Amplitude for recent packet history ring buffer
                    let mut current_amp = [0u8; 64];
                    for i in 0..64 {
                        current_amp[i] = csi::CSI_AMPLITUDES[i].load(Ordering::Relaxed);
                    }
                    packet_ring[ring_idx] = (current_amp, now_ms);
                    ring_idx = (ring_idx + 1) % packet_ring.len();

                    // 2. Snapshot RSSI for history tracking and EMA calculation
                    let t_sec = now_ms.saturating_sub(start_time_ms) as f64 / 1000.0;
                    let rssi_f64 = rssi as f64;

                    if rssi_count < 100 {
                        rssi_history_points[rssi_count] = (t_sec, rssi_f64);
                        let prev = current_ema.unwrap_or(rssi_f64);
                        let next = alpha * rssi_f64 + (1.0 - alpha) * prev;
                        current_ema = Some(next);
                        ema_points[rssi_count] = (t_sec, next);
                        rssi_count += 1;
                    } else {
                        // Shift left
                        for i in 0..99 {
                            rssi_history_points[i] = rssi_history_points[i + 1];
                            ema_points[i] = ema_points[i + 1];
                        }
                        rssi_history_points[99] = (t_sec, rssi_f64);
                        let prev = current_ema.unwrap_or(rssi_f64);
                        let next = alpha * rssi_f64 + (1.0 - alpha) * prev;
                        current_ema = Some(next);
                        ema_points[99] = (t_sec, next);
                    }
                }

                // Shift waterfall
                for i in (1..30).rev() {
                    waterfall_history[i] = waterfall_history[i - 1];
                }

                for i in 0..64 {
                    let amp_val = csi::CSI_AMPLITUDES[i].load(Ordering::Relaxed);
                    waterfall_history[0][i] = amp_val;
                    
                    let phase_f64 = csi::CSI_PHASES[i].load(Ordering::Relaxed) as f64 / 1000.0;
                    csi_phases_f64[i] = (i as f64, phase_f64);
                }
                
                // SD logging...
                if sd_active && csi::NEW_CSV_READY.load(Ordering::Acquire) {
                    let len = csi::CSV_LEN.load(Ordering::Acquire);
                    let mut row_copy = alloc::vec::Vec::with_capacity(len);
                    
                    unsafe {
                        row_copy.extend_from_slice(&csi::CSV_BUFFER[..len]);
                    }
                    csi::NEW_CSV_READY.store(false, Ordering::Release);

                    if let Ok(volume) = volume_mgr.open_volume(VolumeIdx(0)) {
                        if let Ok(rd) = volume.open_root_dir() {
                            if let Ok(f) = rd.open_file_in_dir(filename_str.as_str(), Mode::ReadWriteCreateOrAppend) {
                                let _ = f.write(&row_copy);
                                let _ = f.close();
                            }
                        }
                    }
                }
            }

            // PASS ALL NEW DATA STRUCTURES TO UI RENDERER
            terminal.draw(|f| { 
                render_frame(
                    f, 
                    &current_page, 
                    finger_down, 
                    &csi_phases_f64, 
                    &waterfall_history, 
                    rssi, 
                    active_tab, 
                    packet_count,
                    &packet_ring,
                    now_ms,
                    &rssi_history_points,
                    &ema_points,
                    rssi_count
                ); 
            }).unwrap();
            delay.delay_millis(33);
        }
    }).unwrap();

    let chosen_mode = loop {
        let mode = SELECTED_MODE.load(Ordering::Relaxed);
        if mode != 0 { break mode; }
        Timer::after(Duration::from_millis(100)).await;
    };
    // change ssid & password to the desired network for station
    let client_config = ClientConfig::default().with_ssid("ssid".to_string()).with_password("password".to_string()).with_auth_method(esp_radio::wifi::AuthMethod::Wpa2Personal);

    let node_configuration = match chosen_mode {
        1 => Node::Central(CentralOpMode::WifiStation(WifiStationConfig { client_config })),
        2 => Node::Peripheral(PeripheralOpMode::WifiSniffer(WifiSnifferConfig::default())),
        3 => Node::Central(CentralOpMode::EspNow(EspNowConfig::default())),
        _ => unreachable!(),
    };

    let mut node_handle = CSINodeClient::new();
    let csi_hardware = CSINodeHardware::new(&mut interfaces, controller);
    
    let mut node = CSINode::new(node_configuration, CollectionMode::Collector, Some(CsiConfig::default()), Some(10000), csi_hardware);

    if chosen_mode == 2 || chosen_mode == 3 {
        node.set_protocol(esp_radio::wifi::Protocol::P802D11BGNLR);
        node.set_rate(esp_radio::esp_now::WifiPhyRate::RateMcs0Lgi);
    } else {
        node.set_protocol(esp_radio::wifi::Protocol::P802D11BGN); 
    }

    join(node.run(), node_task(&mut node_handle, chosen_mode)).await;

    loop { Timer::after(Duration::from_secs(1)).await; }
}
