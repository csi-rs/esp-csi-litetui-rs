//! Core-0 CSI consumption.
//!
//! Two delivery paths feed the same live-view atomics:
//! * [`run_drain`] — the async drain task. Pops owned [`CSIDataPacket`]s off
//!   esp-csi-rs's lock-free queue, updates the live view, and (while
//!   recording) serializes each record into [`shared::LOG_RING`] for the
//!   core-1 SD writer. This is the path used for logging.
//! * [`on_csi_callback`] — the inline-callback path (`CsiDeliveryMode::Callback`).
//!   Updates the live view only; no SD logging (it runs on the Wi-Fi hot path).

use core::fmt::Write as _;
use core::f32::consts::PI;
use core::sync::atomic::Ordering;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};
use esp_csi_rs::csi::CSIDataPacket;
use esp_csi_rs::CSINodeClient;
use micromath::F32Ext;

use crate::config::LogFormat;
use crate::shared::{self, NUM_SUBCARRIERS};

/// CSV header written once when a CSV capture file is opened.
pub const CSV_HEADER: &str =
    "host_ms,seq,mac,rssi,noise,rate,mcs,bw,sig_mode,sgi,stbc,fec,ant,channel,sec_ch,fmt,csi_len,csi\n";

/// Update the live per-subcarrier + metadata atomics from a packet.
///
/// Cheap enough to run inline in the Wi-Fi callback (only `micromath`
/// `sqrt`/`atan2`, no heap, no locks).
pub fn update_live(p: &CSIDataPacket) {
    shared::PACKET_COUNT.fetch_add(1, Ordering::Relaxed);
    shared::RSSI.store(p.rssi, Ordering::Relaxed);
    shared::NOISE_FLOOR.store(p.noise_floor, Ordering::Relaxed);
    shared::RX_CHANNEL.store(p.channel as u8, Ordering::Relaxed);
    shared::MCS.store(p.mcs as u8, Ordering::Relaxed);
    shared::BANDWIDTH.store(p.bandwidth as u8, Ordering::Relaxed);
    shared::SIG_MODE.store(p.sig_mode as u8, Ordering::Relaxed);
    shared::RATE.store(p.rate as u16, Ordering::Relaxed);
    shared::SEQUENCE.store(p.sequence_number, Ordering::Relaxed);
    shared::CSI_LEN.store(p.csi_data_len, Ordering::Relaxed);
    shared::DATA_FORMAT.store(p.data_format.clone() as u8, Ordering::Relaxed);

    let raw = p.csi_data();
    let n = core::cmp::min(raw.len() / 2, NUM_SUBCARRIERS);
    let mut prev_phase = 0.0f32;
    let mut unwrap_offset = 0.0f32;
    for i in 0..n {
        let imag = raw[i * 2] as f32;
        let real = raw[i * 2 + 1] as f32;

        let amp = (imag * imag + real * real).sqrt();
        shared::CSI_AMPLITUDES[i].store(amp.min(255.0) as u8, Ordering::Relaxed);

        let mut phase = imag.atan2(real);
        if i > 0 {
            let d = phase - prev_phase;
            if d > PI {
                unwrap_offset -= 2.0 * PI;
            } else if d < -PI {
                unwrap_offset += 2.0 * PI;
            }
        }
        prev_phase = phase;
        phase += unwrap_offset;
        shared::CSI_PHASES[i].store((phase * 1000.0) as i32, Ordering::Relaxed);
    }
    for i in n..NUM_SUBCARRIERS {
        shared::CSI_AMPLITUDES[i].store(0, Ordering::Relaxed);
        shared::CSI_PHASES[i].store(0, Ordering::Relaxed);
    }
}

/// Inline callback path (`CsiDeliveryMode::Callback`). Live view only.
pub fn on_csi_callback(p: &CSIDataPacket) {
    update_live(p);
}

/// Async drain task (`CsiDeliveryMode::Async`). Drives the live view and the
/// SD log ring until the capture is asked to stop.
pub async fn run_drain(client: &mut CSINodeClient) {
    // Exits when `wait_for_stop` wins the select (capture stopped).
    while let Either::First(packet) = select(client.next_csi_packet(), wait_for_stop()).await {
        update_live(&packet);
        if shared::recording_active() {
            let fmt = LogFormat::from_u8(shared::LOG_FORMAT.load(Ordering::Relaxed));
            push_record(&packet, fmt);
        }
    }
}

async fn wait_for_stop() {
    loop {
        if shared::stop_requested() {
            return;
        }
        Timer::after(Duration::from_millis(50)).await;
    }
}

// Single-producer scratch: only `run_drain` (one task) ever touches it.
const SCRATCH_LEN: usize = 3520;
static mut SCRATCH: [u8; SCRATCH_LEN] = [0u8; SCRATCH_LEN];

fn push_record(p: &CSIDataPacket, fmt: LogFormat) {
    // SAFETY: single writer (the drain task).
    let scratch = unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) };
    let pushed = match fmt {
        LogFormat::Binary => match postcard::to_slice_cobs(p, scratch) {
            Ok(record) => shared::LOG_RING.push(record),
            Err(_) => true, // serialization failure isn't a ring drop
        },
        LogFormat::Csv => {
            let n = format_csv(p, scratch);
            shared::LOG_RING.push(&scratch[..n])
        }
    };
    if pushed {
        shared::SD_RECORDS.fetch_add(1, Ordering::Relaxed);
    } else {
        shared::RING_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl core::fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let b = s.as_bytes();
        if self.pos + b.len() > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.pos..self.pos + b.len()].copy_from_slice(b);
        self.pos += b.len();
        Ok(())
    }
}

/// Format one rich CSV row into `buf`, returning the byte count.
fn format_csv(p: &CSIDataPacket, buf: &mut [u8]) -> usize {
    let host_ms = Instant::now().as_millis();
    let m = p.mac;
    let mut w = SliceWriter { buf, pos: 0 };
    let _ = write!(
        &mut w,
        "{},{},{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X},{},{},{},{},{},{},{},{},{},{},{},{},{},[",
        host_ms,
        p.sequence_number,
        m[0], m[1], m[2], m[3], m[4], m[5],
        p.rssi,
        p.noise_floor,
        p.rate,
        p.mcs,
        p.bandwidth,
        p.sig_mode,
        p.sgi,
        p.stbc,
        p.fec_coding,
        p.antenna,
        p.channel,
        p.secondary_channel,
        crate::config::fmt_label(p.data_format.clone() as u8),
    );
    let _ = write!(&mut w, "{},", p.csi_data_len);
    let data = p.csi_data();
    for (i, v) in data.iter().enumerate() {
        if i + 1 < data.len() {
            let _ = write!(&mut w, "{} ", v);
        } else {
            let _ = write!(&mut w, "{}", v);
        }
    }
    let _ = w.write_str("]\n");
    w.pos
}
