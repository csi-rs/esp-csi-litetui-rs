//! Core-1 SD-card logging subsystem.
//!
//! Opens one capture file per session (binary `.BIN` or `.CSV`), keeps the
//! `RawFile` handle open for the whole capture, and appends records as they
//! arrive — a big improvement over re-opening the volume/dir/file per packet.
//! Bytes are drained from [`shared::LOG_RING`] (filled by the core-0 drain
//! task) and written in bulk; the file is flushed after each batch and on
//! finish.

use alloc::string::String;
use core::sync::atomic::Ordering;

use embedded_sdmmc::{
    BlockDevice, Mode, RawDirectory, RawFile, RawVolume, TimeSource, VolumeIdx, VolumeManager,
};

use crate::capture::CSV_HEADER;
use crate::config::LogFormat;
use crate::shared;

struct OpenFile {
    volume: RawVolume,
    dir: RawDirectory,
    file: RawFile,
    name: String,
}

/// Owns the SD volume manager and (when capturing) the open log file.
pub struct SdSink<D: BlockDevice, T: TimeSource>
where
    D::Error: core::fmt::Debug,
{
    mgr: VolumeManager<D, T>,
    open: Option<OpenFile>,
    drain_buf: [u8; 2048],
}

impl<D: BlockDevice, T: TimeSource> SdSink<D, T>
where
    D::Error: core::fmt::Debug,
{
    pub fn new(mgr: VolumeManager<D, T>) -> Self {
        Self {
            mgr,
            open: None,
            drain_buf: [0u8; 2048],
        }
    }

    pub fn is_logging(&self) -> bool {
        self.open.is_some()
    }

    /// Open `CSI<id>.<ext>` for this session. Sets [`shared::SD_STATUS`].
    pub fn start(&mut self, fmt: LogFormat, id: u16) {
        self.open = None;
        let name = alloc::format!("CSI{:05}.{}", id, fmt.extension());
        match self.try_open(&name, fmt) {
            Some(of) => {
                self.open = Some(of);
                shared::SD_STATUS.store(shared::SD_OK, Ordering::Relaxed);
            }
            None => shared::SD_STATUS.store(shared::SD_NO_CARD, Ordering::Relaxed),
        }
    }

    fn try_open(&mut self, name: &str, fmt: LogFormat) -> Option<OpenFile> {
        let volume = self.mgr.open_raw_volume(VolumeIdx(0)).ok()?;
        let dir = self.mgr.open_root_dir(volume).ok()?;
        let file = self
            .mgr
            .open_file_in_dir(dir, name, Mode::ReadWriteCreateOrTruncate)
            .ok()?;
        if fmt == LogFormat::Csv {
            let _ = self.mgr.write(file, CSV_HEADER.as_bytes());
        }
        let _ = self.mgr.flush_file(file);
        Some(OpenFile {
            volume,
            dir,
            file,
            name: String::from(name),
        })
    }

    /// Drain *up to a bounded budget* of pending bytes from the log ring into
    /// the open file, then return.
    ///
    /// Bounded on purpose: core 0 refills the ring continuously during capture,
    /// so an unbounded "drain until empty" loop would never return and would
    /// starve the LCD/touch. Whatever doesn't fit this call is written on the
    /// next frame; if the ring overflows, core 0 counts the dropped records.
    /// Does not flush — see [`flush`](Self::flush).
    pub fn drain(&mut self) {
        let Some(of) = self.open.as_ref() else {
            return;
        };
        let file = of.file;
        let mut budget = DRAIN_BUDGET;
        while budget > 0 {
            let want = self.drain_buf.len().min(budget);
            let n = shared::LOG_RING.pop(&mut self.drain_buf[..want]);
            if n == 0 {
                break;
            }
            if self.mgr.write(file, &self.drain_buf[..n]).is_err() {
                shared::SD_STATUS.store(shared::SD_ERROR, Ordering::Relaxed);
                return;
            }
            budget -= n;
        }
    }

    /// Flush the file (directory entry + FAT). Call on a slow cadence — it is
    /// the expensive part of SD I/O.
    pub fn flush(&mut self) {
        if let Some(of) = self.open.as_ref()
            && self.mgr.flush_file(of.file).is_err()
        {
            shared::SD_STATUS.store(shared::SD_ERROR, Ordering::Relaxed);
        }
    }

    /// Flush + close the file, deleting it when `save` is false.
    ///
    /// Called after the capture has stopped, so the producer is idle and the
    /// remaining ring contents are finite — safe to drain fully here.
    pub fn finish(&mut self, save: bool) {
        if let Some(of) = self.open.as_ref() {
            let file = of.file;
            loop {
                let n = shared::LOG_RING.pop(&mut self.drain_buf);
                if n == 0 {
                    break;
                }
                if self.mgr.write(file, &self.drain_buf[..n]).is_err() {
                    shared::SD_STATUS.store(shared::SD_ERROR, Ordering::Relaxed);
                    break;
                }
            }
        }
        if let Some(of) = self.open.take() {
            let _ = self.mgr.flush_file(of.file);
            let _ = self.mgr.close_file(of.file);
            if !save {
                let _ = self.mgr.delete_file_in_dir(of.dir, of.name.as_str());
            }
            let _ = self.mgr.close_dir(of.dir);
            let _ = self.mgr.close_volume(of.volume);
        }
    }
}

/// Max bytes written to SD per `drain()` call, bounding core-1 stall time.
const DRAIN_BUDGET: usize = 8192;
