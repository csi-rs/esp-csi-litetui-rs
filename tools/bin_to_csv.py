#!/usr/bin/env python3
"""Deserialize CSI LiteTUI binary captures (.BIN) to CSV.

The firmware writes one COBS-framed `postcard` record per CSI packet — the
device's `esp_csi_rs::csi::CSIDataPacket` (ESP32-S3 / non-C5/C6 layout),
serialized with `postcard::to_slice_cobs` and 0x00-delimited. This script
decodes that stream to CSV with no third-party dependencies.

Usage:
    python3 bin_to_csv.py CAPTURE.BIN [OUTPUT.CSV]

If OUTPUT.CSV is omitted it is derived from the input name.

postcard wire format used here:
  * u8 / i8                : 1 raw byte
  * u16 / u32 / u64 / usize: unsigned LEB128 varint
  * i16 / i32 / i64        : zig-zag, then unsigned LEB128 varint
  * fixed array [T; N]     : N elements, no length prefix
  * Option<T>              : 1-byte tag (0 = None, 1 = Some) then T if Some
  * enum                   : variant index as a varint, then the variant data
  * seq (Vec<T>)           : length as a varint, then the elements
"""

import csv
import sys

# RxCSIFmt variants in declaration order (the index postcard stores). The order
# is load-bearing — it must match the upstream enum and `src/config.rs`'s
# `fmt_label` table — so never reorder it.
# This matches the current upstream enum: 15 variants, Undefined at 14. Older
# firmware layouts placed Undefined at 13 (no VhtBw20) or at 16 (two extra
# formats ahead of it), so .BIN files from those builds will decode this column
# wrongly — check which firmware wrote a capture before trusting data_format.
FMT = [
    "Bw20", "HtBw20", "HtBw20Stbc", "SecbBw20", "SecbHtBw20", "SecbHtBw20Stbc",
    "SecbHtBw40", "SecbHtBw40Stbc", "SecaBw20", "SecaHtBw20", "SecaHtBw20Stbc",
    "SecaHtBw40", "SecaHtBw40Stbc", "VhtBw20", "Undefined",
]

COLS = [
    "seq", "mac", "rssi", "noise_floor", "rate", "sgi", "sec_channel",
    "channel", "bandwidth", "antenna", "sig_mode", "mcs", "smoothing",
    "not_sounding", "aggregation", "stbc", "fec_coding", "ampdu_cnt",
    "rx_state", "sig_len", "timestamp_us", "datetime", "data_format",
    "csi_len", "csi",
]


def cobs_decode(frame: bytes) -> bytes:
    """Decode one COBS block (the trailing 0x00 delimiter already stripped)."""
    out = bytearray()
    i, n = 0, len(frame)
    while i < n:
        code = frame[i]
        i += 1
        if code == 0:
            break
        block = code - 1
        out += frame[i:i + block]
        i += block
        if code < 0xFF and i < n:
            out.append(0)
    return bytes(out)


class Reader:
    def __init__(self, data: bytes):
        self.d = data
        self.i = 0

    def u8(self) -> int:
        b = self.d[self.i]
        self.i += 1
        return b

    def i8(self) -> int:
        b = self.u8()
        return b - 256 if b >= 128 else b

    def varint(self) -> int:
        result = 0
        shift = 0
        while True:
            b = self.d[self.i]
            self.i += 1
            result |= (b & 0x7F) << shift
            if not (b & 0x80):
                return result
            shift += 7

    def zigzag(self) -> int:
        u = self.varint()
        return (u >> 1) ^ -(u & 1)

    def take(self, n: int) -> bytes:
        b = self.d[self.i:self.i + n]
        self.i += n
        return b


def decode_record(buf: bytes) -> dict:
    r = Reader(buf)
    rec = {}
    rec["mac"] = ":".join(f"{b:02X}" for b in r.take(6))
    rec["rssi"] = r.zigzag()           # i32
    rec["timestamp_us"] = r.varint()   # u32
    rec["rate"] = r.varint()
    rec["sgi"] = r.varint()
    rec["sec_channel"] = r.varint()
    rec["channel"] = r.varint()
    rec["bandwidth"] = r.varint()
    rec["antenna"] = r.varint()
    rec["sig_mode"] = r.varint()
    rec["mcs"] = r.varint()
    rec["smoothing"] = r.varint()
    rec["not_sounding"] = r.varint()
    rec["aggregation"] = r.varint()
    rec["stbc"] = r.varint()
    rec["fec_coding"] = r.varint()
    rec["ampdu_cnt"] = r.varint()
    rec["noise_floor"] = r.zigzag()    # i32
    rec["rx_state"] = r.varint()
    rec["sig_len"] = r.varint()
    if r.u8() == 1:                    # Option<DateTime> tag
        dt = [r.varint() for _ in range(7)]
        rec["datetime"] = (
            f"{dt[0]:04d}-{dt[1]:02d}-{dt[2]:02d} "
            f"{dt[3]:02d}:{dt[4]:02d}:{dt[5]:02d}.{dt[6]:03d}"
        )
    else:
        rec["datetime"] = ""
    rec["seq"] = r.varint()            # u16
    fmt_idx = r.varint()               # enum variant index
    rec["data_format"] = FMT[fmt_idx] if fmt_idx < len(FMT) else str(fmt_idx)
    rec["csi_len"] = r.varint()        # u16
    n = r.varint()                     # csi_data seq length
    rec["csi"] = " ".join(str(r.i8()) for _ in range(n))
    return rec


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: bin_to_csv.py CAPTURE.BIN [OUTPUT.CSV]", file=sys.stderr)
        return 1
    inp = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else inp.rsplit(".", 1)[0] + ".csv"

    with open(inp, "rb") as f:
        data = f.read()

    rows, ok, bad = [], 0, 0
    for frame in data.split(b"\x00"):
        if not frame:
            continue
        try:
            rows.append(decode_record(cobs_decode(frame)))
            ok += 1
        except Exception:  # noqa: BLE001 - skip truncated/corrupt trailing frame
            bad += 1

    with open(out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=COLS)
        w.writeheader()
        w.writerows(rows)

    print(f"decoded {ok} records ({bad} skipped) -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
