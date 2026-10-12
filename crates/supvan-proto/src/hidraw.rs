//! Low-level hidraw device I/O for USB HID communication.
//!
//! Opens `/dev/hidrawN` and provides 64-byte HID report read/write.

use crate::error::{Error, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::time::Duration;

/// HID report size for the Supvan T50 Pro USB interface.
pub(crate) const HID_REPORT_SIZE: usize = 64;

/// A raw HID device opened via `/dev/hidrawN`.
pub struct HidrawDevice {
    file: File,
}

impl HidrawDevice {
    /// Open a hidraw device by path (e.g. "/dev/hidraw7").
    pub fn open(path: &str) -> Result<Self> {
        let file = File::options().read(true).write(true).open(path)?;
        Ok(Self { file })
    }

    /// Write a 64-byte HID output report, zero-padding short data.
    ///
    /// The device uses no report IDs, but hidraw still expects one as the
    /// first byte — and usbhid strips a leading `0x00` as that ID
    /// (`usbhid_output_report`). Without the explicit `0x00` prefix, any
    /// report whose payload happened to start with a zero byte (common in
    /// LZMA data) went out one byte short and corrupted the stream.
    pub(crate) fn write_report(&self, data: &[u8]) -> Result<()> {
        let mut report = [0u8; 1 + HID_REPORT_SIZE];
        let len = data.len().min(HID_REPORT_SIZE);
        report[1..=len].copy_from_slice(&data[..len]);
        (&self.file).write_all(&report)?;
        Ok(())
    }

    /// Read a HID input report (up to 64 bytes); `None` on timeout.
    pub(crate) fn read_report(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout_ms = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
        let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if ret < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        if ret == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                format!("hidraw poll error: revents=0x{:04x}", pfd.revents),
            )));
        }
        let mut buf = [0u8; HID_REPORT_SIZE];
        let n = (&self.file).read(&mut buf)?;
        Ok((n > 0).then(|| buf[..n].to_vec()))
    }

    /// Discard input reports already queued — stale replies would otherwise
    /// be read as the answer to the next command (the Android app's `clear()`
    /// before every write does the same over SPP).
    pub(crate) fn drain(&self) -> Result<()> {
        while let Some(stale) = self.read_report(Duration::ZERO)? {
            log::debug!(
                "USB drained stale report: {:02x?}",
                &stale[..stale.len().min(8)]
            );
        }
        Ok(())
    }
}
