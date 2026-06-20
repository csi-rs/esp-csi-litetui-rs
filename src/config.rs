//! Capture configuration: compile-time constants, on-device runtime options,
//! and the mapping from the UI's control atomics onto esp-csi-rs config types.

use core::sync::atomic::Ordering;

use esp_csi_rs::config::CsiConfig;
use esp_radio::esp_now::WifiPhyRate;

use crate::shared;

// ---------------------------------------------------------------------------
// Compile-time constants
// ---------------------------------------------------------------------------

/// Wi-Fi network joined in Station mode. Edit and reflash to change.
pub const WIFI_SSID: &str = "Connected Motion ";
/// Password for [`WIFI_SSID`].
pub const WIFI_PASSWORD: &str = "automotion@123";

/// Valid 2.4 GHz primary channels selectable on-device.
pub const MIN_CHANNEL: u8 = 1;
pub const MAX_CHANNEL: u8 = 13;

/// Traffic-generation frequencies (Hz) the config UI steps through.
pub const TRAFFIC_STEPS: [u16; 6] = [10, 50, 100, 500, 1000, 2000];

/// Selectable ESP-NOW PHY rates (applied via `CSINode::set_rate`; only
/// meaningful in the ESP-NOW modes — ignored for Station / Sniffer).
pub const RATE_OPTIONS: [(WifiPhyRate, &str); 8] = [
    (WifiPhyRate::Rate1mL, "1M"),
    (WifiPhyRate::Rate6m, "6M"),
    (WifiPhyRate::Rate24m, "24M"),
    (WifiPhyRate::Rate54m, "54M"),
    (WifiPhyRate::RateMcs0Lgi, "MCS0"),
    (WifiPhyRate::RateMcs3Lgi, "MCS3"),
    (WifiPhyRate::RateMcs5Lgi, "MCS5"),
    (WifiPhyRate::RateMcs7Lgi, "MCS7"),
];
/// Default index into [`RATE_OPTIONS`] (`RateMcs0Lgi`).
pub const RATE_DEFAULT_IDX: u8 = 4;

/// Resolve a stored rate index to a [`WifiPhyRate`].
pub fn rate_from_index(idx: u8) -> WifiPhyRate {
    RATE_OPTIONS
        .get(idx as usize)
        .copied()
        .unwrap_or(RATE_OPTIONS[RATE_DEFAULT_IDX as usize])
        .0
}

/// Label for a stored rate index.
pub fn rate_label(idx: u8) -> &'static str {
    RATE_OPTIONS
        .get(idx as usize)
        .unwrap_or(&RATE_OPTIONS[RATE_DEFAULT_IDX as usize])
        .1
}

// ---------------------------------------------------------------------------
// Node mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Station = 1,
    Sniffer = 2,
    EspNowCentral = 3,
    EspNowPeripheral = 4,
}

impl NodeMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Station),
            2 => Some(Self::Sniffer),
            3 => Some(Self::EspNowCentral),
            4 => Some(Self::EspNowPeripheral),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Station => "Station",
            Self::Sniffer => "Sniffer",
            Self::EspNowCentral => "ESP-NOW Central",
            Self::EspNowPeripheral => "ESP-NOW Periph",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Station => Self::Sniffer,
            Self::Sniffer => Self::EspNowCentral,
            Self::EspNowCentral => Self::EspNowPeripheral,
            Self::EspNowPeripheral => Self::Station,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Station => Self::EspNowPeripheral,
            Self::Sniffer => Self::Station,
            Self::EspNowCentral => Self::Sniffer,
            Self::EspNowPeripheral => Self::EspNowCentral,
        }
    }

    /// `true` for the ESP-NOW modes, which drive traffic at a chosen rate and
    /// use a fixed channel.
    pub fn is_esp_now(self) -> bool {
        matches!(self, Self::EspNowCentral | Self::EspNowPeripheral)
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
    pub rate: WifiPhyRate,
}

impl Config {
    /// Read the live configuration from `shared`'s control atomics.
    pub fn load() -> Self {
        Self {
            mode: NodeMode::from_u8(shared::MODE.load(Ordering::Relaxed))
                .unwrap_or(NodeMode::Sniffer),
            channel: shared::CHANNEL.load(Ordering::Relaxed),
            traffic_hz: shared::TRAFFIC_HZ.load(Ordering::Relaxed),
            csi_flags: shared::CSI_FLAGS.load(Ordering::Relaxed),
            csi_shift: shared::CSI_SHIFT.load(Ordering::Relaxed),
            delivery: DeliveryMode::from_u8(shared::DELIVERY.load(Ordering::Relaxed)),
            rate: rate_from_index(shared::RATE_SEL.load(Ordering::Relaxed)),
        }
    }

    pub fn csi_config(&self) -> CsiConfig {
        csi_config_from(self.csi_flags, self.csi_shift)
    }
}
