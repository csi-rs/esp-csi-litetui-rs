//! Capture configuration: compile-time constants, on-device runtime options,
//! and the mapping from the UI's control atomics onto esp-csi-rs config types.

use core::sync::atomic::{AtomicBool, Ordering};

use esp_csi_rs::config::CsiConfig;
use esp_csi_rs::{HtBandwidth, IOTaskConfig};
use esp_radio::wifi::SecondaryChannel;

use crate::shared;

// ---------------------------------------------------------------------------
// Compile-time constants
// ---------------------------------------------------------------------------

/// Wi-Fi network joined in Station mode. Edit and reflash to change.
pub const WIFI_SSID: &str = "Connected Motion ";
/// Password for [`WIFI_SSID`].
pub const WIFI_PASSWORD: &str = "automotion@123";

/// SSID announced in AP-collector mode (open auth; the built-in DHCP server
/// leases 192.168.13.2 to the associating station).
pub const AP_SSID: &str = "esp-csi-ap";

/// Valid 2.4 GHz primary channels selectable on-device.
pub const MIN_CHANNEL: u8 = 1;
pub const MAX_CHANNEL: u8 = 13;

/// Traffic-generation frequencies (Hz) the config UI steps through. Doubles as
/// the emitter's injection rate (period = 1/Hz).
/// The AP collector's ICMP flood benefits from kHz rates; 8000 is the crate's
/// clamp ceiling and acts as "uncapped".
pub const TRAFFIC_STEPS: [u16; 8] = [10, 50, 100, 500, 1000, 2000, 4000, 8000];

// ---------------------------------------------------------------------------
// Node mode
// ---------------------------------------------------------------------------

/// Selectable node mode.
///
/// The discriminants are persisted as a `u8` (NVS / SD-card config) and read
/// back through [`NodeMode::from_u8`], so a value's meaning is permanent.
/// **Discriminants 3, 4, 5 and 6 are retired and must never be reused.** They were the ESP-NOW
/// modes in a much older build. ESP-NOW is back — `esp-csi-rs` never actually dropped it — but the
/// old numbers cannot come back with it, and the reason is stronger than "they were reused once":
/// old discriminant 4 meant the simplex end that *beacons and then receives*, filed at the time as
/// a central. In the node model that end sources no traffic and is the **peripheral**. A config
/// written by pre-0.11 firmware and read by this one would put the node on the wrong side of the
/// link, and it would look like it worked. `from_u8` rejects 3..=6 so such a config fails safe to
/// the default mode instead. New variants continue from 10.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Station = 1,
    Sniffer = 2,
    /// Self-contained softAP collector: AP + single-lease DHCP server; an
    /// associating station generates the traffic captured as CSI.
    AccessPoint = 7,
    /// TX-only HT20 emitter: loop-injects raw sounding frames on a fixed
    /// channel for a companion collector to measure. Captures no CSI.
    Ht20Emitter = 8,
    /// TX-only HT40 emitter: as [`Self::Ht20Emitter`] but 40 MHz wide, with the
    /// secondary channel taken from the HT40 selector.
    Ht40Emitter = 9,
    /// ESP-NOW central: originates the control traffic and captures the peripheral's replies.
    EspNowCentral = 10,
    /// ESP-NOW peripheral: answers a central's control frames and captures them.
    EspNowPeripheral = 11,
    /// ESP-NOW simplex source: owns all transmit airtime and captures nothing. A central listener,
    /// so like the emitters it renders no live view.
    EspNowSimplexSource = 12,
    /// ESP-NOW simplex peer: beacons until it is found, then goes receive-only. The highest CSI
    /// rate this board can reach.
    EspNowSimplexPeer = 13,
}

impl NodeMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Station),
            2 => Some(Self::Sniffer),
            // 3..=6 are the retired ESP-NOW modes (see the type docs); they
            // fall through to `None` so the caller falls back to the default.
            7 => Some(Self::AccessPoint),
            8 => Some(Self::Ht20Emitter),
            9 => Some(Self::Ht40Emitter),
            10 => Some(Self::EspNowCentral),
            11 => Some(Self::EspNowPeripheral),
            12 => Some(Self::EspNowSimplexSource),
            13 => Some(Self::EspNowSimplexPeer),
            _ => None,
        }
    }

    /// Short display string. Kept within the width budget of the fixed-layout
    /// setup screen and live status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Station => "Station",
            Self::Sniffer => "Sniffer",
            Self::AccessPoint => "AP collector",
            Self::Ht20Emitter => "HT20 emitter",
            Self::Ht40Emitter => "HT40 emitter",
            Self::EspNowCentral => "ESP-NOW central",
            Self::EspNowPeripheral => "ESP-NOW periph",
            Self::EspNowSimplexSource => "Simplex source",
            Self::EspNowSimplexPeer => "Simplex peer",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Station => Self::Sniffer,
            Self::Sniffer => Self::AccessPoint,
            Self::AccessPoint => Self::Ht20Emitter,
            Self::Ht20Emitter => Self::Ht40Emitter,
            Self::Ht40Emitter => Self::EspNowCentral,
            Self::EspNowCentral => Self::EspNowPeripheral,
            Self::EspNowPeripheral => Self::EspNowSimplexSource,
            Self::EspNowSimplexSource => Self::EspNowSimplexPeer,
            Self::EspNowSimplexPeer => Self::Station,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Station => Self::EspNowSimplexPeer,
            Self::Sniffer => Self::Station,
            Self::AccessPoint => Self::Sniffer,
            Self::Ht20Emitter => Self::AccessPoint,
            Self::Ht40Emitter => Self::Ht20Emitter,
            Self::EspNowCentral => Self::Ht40Emitter,
            Self::EspNowPeripheral => Self::EspNowCentral,
            Self::EspNowSimplexSource => Self::EspNowPeripheral,
            Self::EspNowSimplexPeer => Self::EspNowSimplexSource,
        }
    }

    /// `true` for the emitter modes, which take a fixed channel and an
    /// injection period rather than joining/serving a network.
    pub fn is_emitter(self) -> bool {
        matches!(self, Self::Ht20Emitter | Self::Ht40Emitter)
    }

    /// `true` when the mode produces local CSI. An emitter only transmits, so it captures nothing
    /// at all — there is no CSI to deliver and no live view to render for it. The simplex source
    /// is the same case under a different name: a central listener.
    pub fn captures_csi(self) -> bool {
        !self.is_emitter() && !matches!(self, Self::EspNowSimplexSource)
    }

    /// TX bandwidth for an emitter, given the stored HT40 selector. Only
    /// meaningful for the emitter modes. `Ht40Emitter` is 40 MHz by definition,
    /// so an "off" selector is read as Above rather than dropping to HT20.
    pub fn emitter_bandwidth(self, ht40: Option<SecondaryChannel>) -> HtBandwidth {
        match (self, ht40) {
            (Self::Ht40Emitter, Some(SecondaryChannel::Below)) => HtBandwidth::Ht40Below,
            (Self::Ht40Emitter, _) => HtBandwidth::Ht40Above,
            _ => HtBandwidth::Ht20,
        }
    }

    /// I/O task split. An emitter is TX-only; a collector keeps the default
    /// TX+RX. Re-applied every capture because the node is reused.
    pub fn io_tasks(self) -> IOTaskConfig {
        if self.is_emitter() || matches!(self, Self::EspNowSimplexSource) {
            IOTaskConfig::new(true, false)
        } else {
            IOTaskConfig::default()
        }
    }
}

/// Mode the device falls back to when a stored mode byte can't be resolved.
pub const DEFAULT_MODE: NodeMode = NodeMode::Sniffer;

/// Latches the retired-discriminant notice. `mode_from_stored` runs on every UI
/// frame, so an unlatched line would flood the serial port.
static MIGRATION_LOGGED: AtomicBool = AtomicBool::new(false);

/// Resolve a persisted mode byte, falling back to [`DEFAULT_MODE`].
///
/// A config saved by pre-emitter firmware can still carry one of the retired
/// ESP-NOW discriminants (see [`NodeMode`]). Those no longer resolve, so note
/// the substitution once instead of letting the device look like it silently
/// ignored the saved mode.
pub fn mode_from_stored(v: u8) -> NodeMode {
    match NodeMode::from_u8(v) {
        Some(mode) => mode,
        None => {
            if matches!(v, 3..=6) && !MIGRATION_LOGGED.swap(true, Ordering::Relaxed) {
                esp_println::println!(
                    "config: stored node mode {} is a retired ESP-NOW mode; falling back to {}",
                    v,
                    DEFAULT_MODE.label()
                );
            }
            DEFAULT_MODE
        }
    }
}

// ---------------------------------------------------------------------------
// Delivery mode / log format
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// Drain owned packets on a core-0 task — required for SD logging.
    Async = 0,
    /// Inline callback (lowest latency, live-view only, no SD logging).
    Callback = 1,
}

impl DeliveryMode {
    pub fn from_u8(v: u8) -> Self {
        if v == 1 { Self::Callback } else { Self::Async }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Async => "Async drain",
            Self::Callback => "Inline cb",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// postcard-serialized `CSIDataPacket`, COBS-framed.
    Binary = 0,
    /// Rich CSV (one row per packet).
    Csv = 1,
}

impl LogFormat {
    pub fn from_u8(v: u8) -> Self {
        if v == 1 { Self::Csv } else { Self::Binary }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Binary => "Binary (.BIN)",
            Self::Csv => "CSV (.CSV)",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Binary => "BIN",
            Self::Csv => "CSV",
        }
    }
}

// ---------------------------------------------------------------------------
// CSI sub-config flags (stored packed in `shared::CSI_FLAGS`)
// ---------------------------------------------------------------------------

pub const CSI_LLTF: u8 = 1 << 0;
pub const CSI_HTLTF: u8 = 1 << 1;
pub const CSI_STBC_HTLTF2: u8 = 1 << 2;
pub const CSI_LTF_MERGE: u8 = 1 << 3;
pub const CSI_CHANNEL_FILTER: u8 = 1 << 4;
pub const CSI_MANU_SCALE: u8 = 1 << 5;

/// Default flag set (matches `CsiConfig::default()` for the ESP32-S3).
pub const CSI_FLAGS_DEFAULT: u8 = CSI_LLTF | CSI_HTLTF | CSI_STBC_HTLTF2 | CSI_LTF_MERGE;

// ---------------------------------------------------------------------------
// HT40 secondary-channel selection (stored in `shared::HT40_SEL`)
// ---------------------------------------------------------------------------

/// Map the stored HT40 selector to a secondary channel, consumed by the AP
/// collector and the HT40 emitter. `None` means HT20 — the secondary channel
/// must then be left unset entirely, because passing `SecondaryChannel::None`
/// still flags the node HT40 and wedges RX (known esp-csi-rs pitfall).
pub fn ht40_from_u8(v: u8) -> Option<SecondaryChannel> {
    match v {
        1 => Some(SecondaryChannel::Above),
        2 => Some(SecondaryChannel::Below),
        _ => None,
    }
}

/// Label for a stored HT40 selector.
pub fn ht40_label(v: u8) -> &'static str {
    match v {
        1 => "Above",
        2 => "Below",
        _ => "Off (HT20)",
    }
}

/// `RxCSIFmt` variant names in declaration order (the discriminant postcard
/// stores). Must stay aligned with esp-csi-rs and `tools/bin_to_csv.py`.
///
/// This mirrors the upstream enum exactly: 15 variants, `Undefined` at 14. An
/// older layout carried two extra formats ahead of `Undefined`, pushing it to
/// 16; `.BIN` files written by that firmware therefore store a different index
/// for `Undefined` and are not decodable with this table. Matching the running
/// firmware is the priority — keeping the old slots would mislabel every
/// `Undefined` frame from current firmware as a placeholder.
pub fn fmt_label(v: u8) -> &'static str {
    const NAMES: [&str; 15] = [
        "Bw20",
        "HtBw20",
        "HtBw20Stbc",
        "SecbBw20",
        "SecbHtBw20",
        "SecbHtBw20Stbc",
        "SecbHtBw40",
        "SecbHtBw40Stbc",
        "SecaBw20",
        "SecaHtBw20",
        "SecaHtBw20Stbc",
        "SecaHtBw40",
        "SecaHtBw40Stbc",
        "VhtBw20",
        "Undefined",
    ];
    NAMES.get(v as usize).copied().unwrap_or("?")
}

/// Build an esp-csi-rs [`CsiConfig`] from the packed flags + shift.
pub fn csi_config_from(flags: u8, shift: u8) -> CsiConfig {
    CsiConfig {
        lltf_en: flags & CSI_LLTF != 0,
        htltf_en: flags & CSI_HTLTF != 0,
        stbc_htltf2_en: flags & CSI_STBC_HTLTF2 != 0,
        ltf_merge_en: flags & CSI_LTF_MERGE != 0,
        channel_filter_en: flags & CSI_CHANNEL_FILTER != 0,
        manu_scale: flags & CSI_MANU_SCALE != 0,
        shift: shift.min(15),
        dump_ack_en: false,
    }
}

/// Snapshot of the current on-device configuration, read from the control atomics.
#[derive(Clone, Copy)]
pub struct Config {
    pub mode: NodeMode,
    pub channel: u8,
    pub traffic_hz: u16,
    pub csi_flags: u8,
    pub csi_shift: u8,
    pub delivery: DeliveryMode,
    /// HT40 secondary channel (`None` = HT20).
    pub ht40: Option<SecondaryChannel>,
}

impl Config {
    /// Read the live configuration from `shared`'s control atomics.
    pub fn load() -> Self {
        Self {
            mode: mode_from_stored(shared::MODE.load(Ordering::Relaxed)),
            channel: shared::CHANNEL.load(Ordering::Relaxed),
            traffic_hz: shared::TRAFFIC_HZ.load(Ordering::Relaxed),
            csi_flags: shared::CSI_FLAGS.load(Ordering::Relaxed),
            csi_shift: shared::CSI_SHIFT.load(Ordering::Relaxed),
            delivery: DeliveryMode::from_u8(shared::DELIVERY.load(Ordering::Relaxed)),
            ht40: ht40_from_u8(shared::HT40_SEL.load(Ordering::Relaxed)),
        }
    }

    pub fn csi_config(&self) -> CsiConfig {
        csi_config_from(self.csi_flags, self.csi_shift)
    }
}
