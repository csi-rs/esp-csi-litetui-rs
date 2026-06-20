//! Cross-core shared state.
//!
//! The application runs the Wi-Fi / CSI pipeline on core 0 (async, esp-rtos +
//! embassy) and the LCD + touch + SD-card I/O on core 1 (a plain blocking
//! loop). All communication between the two cores goes through this module:
//!
//! * **Live atomics** — per-subcarrier amplitude/phase plus packet metadata,
//!   written by the core-0 CSI drain task and read by the core-1 renderer.
//! * **Control atomics** — capture mode / channel / traffic-rate / CSI-config /
//!   delivery-mode / log-format, written by the core-1 config UI and read by
//!   core 0 before a capture starts.
//! * **[`LOG_RING`]** — a single-producer / single-consumer byte ring that
//!   carries already-serialized log records (postcard+COBS or CSV) from the
//!   core-0 drain task to the core-1 SD writer, so slow SD I/O never stalls
//!   CSI capture.
//!
//! Only atomics ≤ 32 bits are used (the Xtensa LX7 has no native 64-bit
//! atomics).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicI32, AtomicU16, AtomicU32, AtomicU8, AtomicUsize, Ordering};

/// Number of subcarrier bins the UI tracks (covers HT20 CSI tone count).
pub const NUM_SUBCARRIERS: usize = 64;

// ---------------------------------------------------------------------------
// Live per-subcarrier view (core 0 -> core 1)
// ---------------------------------------------------------------------------

// Per-element initializers for the atomic arrays below. The interior-mutable
// const is the standard idiom for `[Atomic_; N]` initialization.
#[allow(clippy::declare_interior_mutable_const)]
const AMP_INIT: AtomicU8 = AtomicU8::new(0);
#[allow(clippy::declare_interior_mutable_const)]
const PHASE_INIT: AtomicI32 = AtomicI32::new(0);

/// Per-subcarrier amplitude magnitude (0..=255), `sqrt(I^2 + Q^2)`.
pub static CSI_AMPLITUDES: [AtomicU8; NUM_SUBCARRIERS] = [AMP_INIT; NUM_SUBCARRIERS];
/// Per-subcarrier unwrapped phase in milli-radians (so it fits an `i32`).
pub static CSI_PHASES: [AtomicI32; NUM_SUBCARRIERS] = [PHASE_INIT; NUM_SUBCARRIERS];

// ---------------------------------------------------------------------------
// Live packet metadata (core 0 -> core 1)
// ---------------------------------------------------------------------------

pub static RSSI: AtomicI32 = AtomicI32::new(-100);
pub static NOISE_FLOOR: AtomicI32 = AtomicI32::new(0);
pub static RX_CHANNEL: AtomicU8 = AtomicU8::new(0);
pub static MCS: AtomicU8 = AtomicU8::new(0);
pub static BANDWIDTH: AtomicU8 = AtomicU8::new(0);
pub static SIG_MODE: AtomicU8 = AtomicU8::new(0);
pub static RATE: AtomicU16 = AtomicU16::new(0);
pub static SEQUENCE: AtomicU16 = AtomicU16::new(0);
pub static CSI_LEN: AtomicU16 = AtomicU16::new(0);
/// Total CSI packets observed by the drain task (monotonic).
pub static PACKET_COUNT: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// Capture control (core 1 -> core 0)
// ---------------------------------------------------------------------------

/// Selected node mode: `0` = unset, `1` = station, `2` = sniffer,
/// `3` = ESP-NOW central, `4` = ESP-NOW peripheral.
pub static MODE: AtomicU8 = AtomicU8::new(0);

/// Run state machine:
/// `0` = configuring, `1` = running, `2` = stop requested (save prompt),
/// `3` = finished/saved, `4` = finished/discarded.
pub static RUN: AtomicU8 = AtomicU8::new(0);

pub const RUN_RUNNING: u8 = 1;
pub const RUN_STOP_REQ: u8 = 2;
pub const RUN_SAVED: u8 = 3;
pub const RUN_DISCARDED: u8 = 4;

/// Configured primary Wi-Fi channel (1..=14).
pub static CHANNEL: AtomicU8 = AtomicU8::new(1);
/// ESP-NOW / station traffic-generation frequency in Hz.
pub static TRAFFIC_HZ: AtomicU16 = AtomicU16::new(100);
/// Selected ESP-NOW PHY rate, as an index into `config::RATE_OPTIONS`.
pub static RATE_SEL: AtomicU8 = AtomicU8::new(4);
/// CSI sub-config bit flags (see [`crate::config`]).
pub static CSI_FLAGS: AtomicU8 = AtomicU8::new(0x0F);
/// Manual scaling shift (0..=15), only meaningful when the manual-scale flag is set.
pub static CSI_SHIFT: AtomicU8 = AtomicU8::new(0);
/// CSI delivery mode: `0` = async drain (logging works), `1` = inline callback.
pub static DELIVERY: AtomicU8 = AtomicU8::new(0);
/// SD log format: `0` = binary (postcard+COBS), `1` = CSV.
pub static LOG_FORMAT: AtomicU8 = AtomicU8::new(0);

// ---------------------------------------------------------------------------
// SD-card status (core 1 -> core 1, surfaced in the UI)
// ---------------------------------------------------------------------------

/// SD status: `0` = unknown, `1` = logging OK, `2` = no card, `3` = write error.
pub static SD_STATUS: AtomicU8 = AtomicU8::new(0);
pub const SD_OK: u8 = 1;
pub const SD_NO_CARD: u8 = 2;
pub const SD_ERROR: u8 = 3;

/// Records dropped because [`LOG_RING`] was full (capture outran SD I/O).
pub static RING_DROPS: AtomicU32 = AtomicU32::new(0);
/// Records successfully written to the SD file this session.
pub static SD_RECORDS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Control helpers
// ---------------------------------------------------------------------------

#[inline]
pub fn run_state() -> u8 {
    RUN.load(Ordering::Acquire)
}

/// Core 1: begin a capture with the chosen mode.
pub fn start_capture(mode: u8) {
    MODE.store(mode, Ordering::Relaxed);
    RUN.store(RUN_RUNNING, Ordering::Release);
}

/// Core 1: request the running capture to stop (moves to the save prompt).
pub fn request_stop() {
    RUN.store(RUN_STOP_REQ, Ordering::Release);
}

/// Core 1: return to the configuration state, ready for the next capture.
pub fn to_config() {
    RUN.store(0, Ordering::Release);
}

/// `true` while CSI records should be pushed to the SD log ring.
#[inline]
pub fn recording_active() -> bool {
    RUN.load(Ordering::Acquire) == RUN_RUNNING
}

/// `true` once the capture has been asked to stop (or has finished).
#[inline]
pub fn stop_requested() -> bool {
    RUN.load(Ordering::Acquire) >= RUN_STOP_REQ
}

// ---------------------------------------------------------------------------
// Lock-free SPSC byte ring (core 0 producer -> core 1 consumer)
// ---------------------------------------------------------------------------

const RING_SIZE: usize = 32 * 1024;

/// Single-producer / single-consumer byte ring.
///
/// The core-0 drain task is the only producer ([`push`](Self::push)); the
/// core-1 SD writer is the only consumer ([`pop`](Self::pop)). Records are
/// pushed whole-or-not-at-all so framing is never split. Raw-pointer copies
/// touch only the disjoint producer/consumer regions, and the `head`/`tail`
/// `Acquire`/`Release` pair orders the data against the index publish.
pub struct ByteRing {
    buf: UnsafeCell<[u8; RING_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// SAFETY: access is disciplined SPSC — see the type docs.
unsafe impl Sync for ByteRing {}

impl ByteRing {
    const fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0; RING_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: push `data` as one record, or drop it whole if it doesn't
    /// fit. Returns `true` if the record was stored.
    pub fn push(&self, data: &[u8]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = (tail + RING_SIZE - head - 1) % RING_SIZE;
        if data.len() > free {
            return false;
        }
        let base = self.buf.get() as *mut u8;
        let first = core::cmp::min(data.len(), RING_SIZE - head);
        // SAFETY: producer owns [head, tail-1); writes stay inside it.
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), base.add(head), first);
            let rest = data.len() - first;
            if rest > 0 {
                core::ptr::copy_nonoverlapping(data.as_ptr().add(first), base, rest);
            }
        }
        self.head
            .store((head + data.len()) % RING_SIZE, Ordering::Release);
        true
    }

    /// Consumer: copy up to `out.len()` bytes out of the ring, returning the
    /// number copied (0 if empty).
    pub fn pop(&self, out: &mut [u8]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let avail = (head + RING_SIZE - tail) % RING_SIZE;
        let n = core::cmp::min(avail, out.len());
        if n == 0 {
            return 0;
        }
        let base = self.buf.get() as *const u8;
        let first = core::cmp::min(n, RING_SIZE - tail);
        // SAFETY: consumer owns [tail, head); reads stay inside it.
        unsafe {
            core::ptr::copy_nonoverlapping(base.add(tail), out.as_mut_ptr(), first);
            let rest = n - first;
            if rest > 0 {
                core::ptr::copy_nonoverlapping(base, out.as_mut_ptr().add(first), rest);
            }
        }
        self.tail.store((tail + n) % RING_SIZE, Ordering::Release);
        n
    }
}

/// The shared SD log ring.
pub static LOG_RING: ByteRing = ByteRing::new();
