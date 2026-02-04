//! Low-level hidraw device I/O for USB HID communication.
//!
//! Opens `/dev/hidrawN` and provides 64-byte HID report read/write.

use crate::error::{Error, Result};
use std::ffi::CString;
use std::os::unix::io::RawFd;
use std::time::Duration;

/// HID report size for the Supvan T50 Pro USB interface.
pub const HID_REPORT_SIZE: usize = 64;

/// A raw HID device opened via `/dev/hidrawN`.
pub struct HidrawDevice {
    fd: RawFd,
}

impl HidrawDevice {
    /// Open a hidraw device by path (e.g. "/dev/hidraw7").
    pub fn open(path: &str) -> Result<Self> {
        let c_path = CString::new(path)
            .map_err(|_| Error::InvalidParam(format!("invalid hidraw path: {path}")))?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(Self { fd })
    }

    /// Return the raw file descriptor.
    pub fn raw_fd(&self) -> RawFd {
        self.fd
    }

    /// Write a HID output report. Data is padded with zeros to 64 bytes.
    pub fn write_report(&self, data: &[u8]) -> Result<()> {
        let mut report = [0u8; HID_REPORT_SIZE];
        let copy_len = data.len().min(HID_REPORT_SIZE);
        report[..copy_len].copy_from_slice(&data[..copy_len]);

        let n = unsafe {
            libc::write(
                self.fd,
                report.as_ptr() as *const libc::c_void,
                HID_REPORT_SIZE,
            )
        };
        if n < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Read a HID input report (up to 64 bytes) with a timeout.
    ///
    /// Uses `poll()` to wait for data. Returns `None` on timeout.
    pub fn read_report(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout_ms = timeout.as_millis() as libc::c_int;
        let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if ret < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        if ret == 0 {
            return Ok(None); // timeout
        }

        let mut buf = [0u8; HID_REPORT_SIZE];
        let n = unsafe {
            libc::read(
                self.fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                HID_REPORT_SIZE,
            )
        };
        if n < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(buf[..n as usize].to_vec()))
    }
}

impl Drop for HidrawDevice {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}
