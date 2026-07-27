# Rework: SD logging, faster touch, esp-csi-rs 0.8 / esp-hal 1.1 integration

Merges `dev/rework` → `main`.

## Summary

A ground-up rework of CSI LiteTUI. The previous single-blob app is split into
focused modules, the capture/logging pipeline is rebuilt for reliable on-device
SD-card recording, touch handling is reworked for responsiveness, and the whole
`esp-*` dependency stack is bumped to the versions required by `esp-csi-rs 0.8.3`
— including its new fast ESP-NOW simplex, softAP-collector, and HT40 modes.

Net diff: **+4,849 / −1,227** across 18 files (`src/` restructured into modules,
plus a new offline decode tool and full README rewrite).

## Highlights

### Dual-core architecture
- **Core 0** (async, `esp-rtos` + embassy) runs the Wi-Fi / CSI pipeline.
- **Core 1** (`CpuControl::start_app_core`) owns the LCD, touch panel, and SD card,
  running the scientific UI as a plain blocking loop.
- Cores communicate only through `src/shared.rs`: live atomics for the view,
  control atomics for config, and a lock-free SPSC byte ring (`LOG_RING`) carrying
  pre-serialized log records so slow SD I/O never stalls CSI capture.
  (Only ≤32-bit atomics — the Xtensa LX7 has no native 64-bit atomics.)

### Proper SD-card logging
- File is opened **once per session** and written incrementally (vs. the old
  re-open-per-packet path). Flushed after each batch and on finish.
- **Primary format:** compact binary — `postcard`-serialized `CSIDataPacket`,
  COBS-framed (`CSInnnnn.BIN`).
- **Secondary format:** rich CSV (`CSInnnnn.CSV`).
- Live stats surface dropped records and SD status.
- New `tools/bin_to_csv.py` decodes the binary logs offline.

### Reworked touch & UI
- Faster touch response; single-touch input model that returns an `Action`
  the core-1 loop turns into capture/SD operations.
- UI restructured into `src/ui/{mod,tabs,config_ui}.rs`.
- Scientific instrument tabs with real units/axes: **Spectrum**, **Phase**
  (unwrapped, rad), **Waterfall** (amplitude heatmap), **Signal** (RSSI/SNR
  trend), and **Stats** (live PPS / RX rate / totals / dropped + last-packet
  PHY/BW/MCS/rate/noise-floor metadata).
- On-device config screen for node mode, channel, traffic rate, CSI sub-options
  (L-LTF / HT-LTF / STBC-HT-LTF2 / LTF-merge / channel-filter / manual scale+shift),
  delivery mode, and log format.

### CSI delivery modes
- `Async drain` — full SD logging path.
- `Callback` — inline, lowest-latency live view (runs on the Wi-Fi hot path, no SD).
- Supports all `esp-csi-rs` node modes: Wi-Fi Station, Wi-Fi Sniffer,
  ESP-NOW Central, ESP-NOW Peripheral, ESP-NOW Fast Collector / Fast Source,
  AP collector.

### esp-csi-rs 0.8.x feature integration
- **ESP-NOW fast simplex** (`EspNowFastCollector` / `EspNowFastSource`):
  one-to-one forced-PHY flood for maximum CSI packets/sec. The source runs as a
  TX-only Listener (per-mode `CollectionMode` / `IOTaskConfig`, re-applied every
  capture since the node is reused); the Stats tab now shows TX PPS/total so the
  source side is observable.
- **AP collector** (`WifiAccessPoint`): self-contained softAP (`esp-csi-ap`,
  open auth) + single-lease DHCP server; an associating station generates the
  ping traffic captured as CSI. Traffic steps extended to 4 kHz / 8 kHz.
- **HT40 capture**: new "HT40 sec" setup field (Off / Above / Below) wires
  `EspNowConfig::with_ht40`; UI subcarrier range doubled to 128 bins.
- **Frame-format classification**: 0.8.x computes `data_format` per packet on
  the S3 (always `Undefined` in 0.7). Shown on the Stats tab and logged as a new
  `fmt` CSV column; `tools/bin_to_csv.py` gains the three new `RxCSIFmt`
  variants (`Undefined` moved 13 → 16 — old .BIN files decode index 13 as
  `VhtBw20`; treat it as Undefined).
- `install_static_espnow_recv()` at boot closes the ESP-NOW heap-exhaustion
  startup window.

### Dependency stack bump (matched to `esp-csi-rs 0.8.3`)
| Crate | Old | New |
|-------|-----|-----|
| esp-csi-rs | 0.4.2 | 0.8.3 |
| esp-hal | 1.0.0 | 1.1 |
| esp-rtos | 0.2.0 | 0.3.0 |
| esp-radio | 0.17.0 | 0.18.0 (`+esp-now`, `+unstable`) |
| esp-alloc | 0.9.0 | 0.10.0 |
| esp-backtrace | 0.18.1 | 0.19.0 |
| esp-println | 0.16.1 | 0.17.0 |
| esp-bootloader-esp-idf | 0.4.0 | 0.5.0 |
| embassy-executor | 0.9.1 | 0.10.0 |

- Added `postcard`, `serde` (no-std), `embedded-sdmmc` for binary logging.
- `Cargo.lock` now committed for reproducible builds.
- Build profile: `opt-level = 3` for all deps in dev; `codegen-units = 1` +
  fat LTO in release.

## File changes

**New modules:** `src/board.rs`, `src/capture.rs`, `src/config.rs`,
`src/logging.rs`, `src/shared.rs`, `src/ui/{mod,tabs,config_ui}.rs`,
`tools/bin_to_csv.py`, `Cargo.lock`.

**Removed:** `src/app.rs`, `src/csi.rs`, `src/ui.rs` (folded into the new
module layout); `readme.md` → rewritten as `README.md`.

**Reworked:** `src/main.rs` (740-line rewrite as the dual-core orchestrator),
`Cargo.toml` (deps + version 0.1.0 → 0.2.0, license, profiles).

## Notes for reviewers
- Register-level board workarounds in `src/board.rs` (shared LCD/SD SPI bus,
  DC-line routing on the ILI9342C) are deliberate and hardware-dependent.
- Wi-Fi SSID/password are compile-time constants in `src/config.rs`
  (`WIFI_SSID` / `WIFI_PASSWORD`) — edit and reflash to change.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
