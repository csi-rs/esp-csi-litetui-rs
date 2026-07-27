<div align="center">

# 📡 CSI LiteTUI

### A handheld Wi-Fi CSI scope for the M5Stack CoreS3 SE

Real-time on-device Channel State Information (CSI) capture, scientific
visualization, and SD-card logging — built in Rust on `esp-csi-rs 0.8`,
`esp-hal 1.1`, and `esp-radio 0.18`.

</div>

---

## Overview

CSI LiteTUI turns an **M5Stack CoreS3 SE** (ESP32-S3, 320×240 touchscreen)
into a portable Wi-Fi CSI instrument. It captures CSI through
[`esp-csi-rs`](https://github.com/csi-rs/esp-csi-rs), renders live scientific
views to the LCD, and logs every packet to a microSD card in a compact binary
format (or CSV) for off-line analysis.

## Features

* **All `esp-csi-rs` node modes**, selectable on-device:
  Wi-Fi **Station**, Wi-Fi **Sniffer**, **ESP-NOW Central**, **ESP-NOW
  Peripheral**, **ESP-NOW Fast Collector / Source** (one-to-one forced-PHY
  simplex for maximum CSI packets/sec), and **AP collector** (self-contained
  softAP + DHCP; an associating station generates the captured traffic).
* **HT40 capture** for the ESP-NOW modes: pick the secondary channel
  (Above/Below) on the setup screen for ~117–128 subcarriers instead of ~56.
  2.4 GHz HT40 is experimental upstream — the channel 6 + Above pair works best;
  confirm on-air via the CSI length on the Stats tab.
* **Scientific instrument tabs** (real units/axes, no decorative chrome):
  1. **Spectrum** — amplitude vs subcarrier
  2. **Phase** — unwrapped phase (rad) vs subcarrier
  3. **Waterfall** — amplitude heatmap (time × subcarrier)
  4. **Signal** — RSSI and SNR (`rssi − noise_floor`) trend vs time
  5. **Stats** — live `esp-csi-rs` statistics (RX/TX PPS, rate, total, dropped)
     plus last-packet metadata (PHY, BW, MCS, rate, frame format classification,
     noise floor, sequence, CSI length)
* **On-device configuration** (touch): node mode, channel, traffic rate,
  HT40 secondary channel, CSI sub-options (L-LTF / HT-LTF / STBC-HT-LTF2 /
  LTF-merge / channel-filter / manual-scale + shift), delivery mode, and log
  format.
* **CSI delivery toggle** — `Async drain` (full SD logging) or inline
  `Callback` (lowest-latency live view).
* **Proper SD logging** — the file is opened once per session and written
  incrementally; live statistics surface dropped records and SD status.
  * **Primary:** compact **binary** — `postcard`-serialized `CSIDataPacket`,
    COBS-framed (`CSInnnnn.BIN`).
  * **Secondary:** rich **CSV** (`CSInnnnn.CSV`).

## Hardware

| Function | GPIO | Notes |
|----------|------|-------|
| I2C SDA / SCL | 12 / 11 | PMIC `0x34`, IO-expander `0x58`, touch `0x38` |
| SPI SCK / MOSI / MISO | 36 / 37 / 35 | shared LCD + SD bus |
| LCD CS / SD CS | 3 / 4 | |

Display: ILI9342C (BGR, inverted). Board-specific register workarounds for the
DC line and shared MISO routing live in [`src/board.rs`](src/board.rs).

## Build & flash

Requires the Espressif Rust toolchain (`espup`) and `espflash`.

```bash
# Station mode joins this network — edit before flashing:
#   src/config.rs : WIFI_SSID / WIFI_PASSWORD

cargo build --release
cargo run --release        # builds, flashes, and opens the serial monitor
```

Target (`xtensa-esp32s3-none-elf`), runner, and `build-std` are preconfigured in
[`.cargo/config.toml`](.cargo/config.toml).

## On-device usage

1. **Setup screen** — tap the top-left / top-right to move between fields, the
   middle-left / middle-right to change a value, and **START CAPTURE** at the
   bottom to begin.
2. **Live screen** — tap **NEXT** (top-right) to cycle instrument tabs, **STOP**
   (top-left) to end the capture.
3. **Save prompt** — **KEEP** keeps the SD file, **DELETE** removes it.

The card must be formatted **FAT32** (or FAT16); exFAT is not supported.

## SD output formats

Set the log format on the setup screen.

* **Binary (`CSInnnnn.BIN`) — default.** Compact COBS-framed `postcard` records,
  one per packet. Convert to CSV with the bundled script (pure Python, no
  dependencies):

  ```bash
  python3 tools/bin_to_csv.py CAPTURE.BIN        # -> CAPTURE.csv
  ```

* **CSV (`CSInnnnn.CSV`).** Human-readable, one row per packet; the `fmt` column
  carries the `RxCSIFmt` frame classification and the `csi` column is a
  space-separated list of the raw CSI samples.

## License

Copyright 2026 The esp-csi Team
Apache-2.0. See [LICENSE](LICENSE).

---

Made with 🦀 for ESP chips