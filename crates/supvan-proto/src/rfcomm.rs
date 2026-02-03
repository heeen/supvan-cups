//! Raw Bluetooth RFCOMM socket using libc.
//!
//! Direct port of SupvanPrinter._connect_bt() / BluetoothUtils.ConnectedThread
//! from the Python reference. Uses AF_BLUETOOTH + SOCK_STREAM + BTPROTO_RFCOMM.

use crate::error::{Error, Result};
use std::os::unix::io::RawFd;
use std::time::Duration;

// Bluetooth socket constants (from <bluetooth/bluetooth.h> and <bluetooth/rfcomm.h>)
const AF_BLUETOOTH: i32 = 31;
const BTPROTO_RFCOMM: i32 = 3;
const RFCOMM_DEFAULT_CHANNEL: u8 = 1;

/// sockaddr_rc structure for RFCOMM connections.
#[repr(C)]
struct SockaddrRc {
    rc_family: u16,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

/// A Bluetooth RFCOMM socket connection to a printer.
pub struct RfcommSocket {
    fd: RawFd,
    timeout: Duration,
}

impl RfcommSocket {
    /// Return the raw file descriptor for this socket.
    pub fn raw_fd(&self) -> RawFd {
        self.fd
    }

    /// Connect to a Bluetooth device by address string (e.g. "A4:93:40:A0:87:57").
    pub fn connect(addr: &str, channel: u8) -> Result<Self> {
        let bdaddr = parse_bdaddr(addr)?;

        let fd = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
        if fd < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }

        let sa = SockaddrRc {
            rc_family: AF_BLUETOOTH as u16,
            rc_bdaddr: bdaddr,
            rc_channel: channel,
        };

        let ret = unsafe {
            libc::connect(
                fd,
                &sa as *const SockaddrRc as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(Error::Io(err));
        }

        let sock = Self {
            fd,
            timeout: Duration::from_secs(2),
        };
        sock.set_timeout(Duration::from_secs(2))?;
        Ok(sock)
    }

    /// Connect with default RFCOMM channel 1.
    pub fn connect_default(addr: &str) -> Result<Self> {
        Self::connect(addr, RFCOMM_DEFAULT_CHANNEL)
    }

    fn set_timeout(&self, timeout: Duration) -> Result<()> {
        let tv = libc::timeval {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_usec: timeout.subsec_micros() as libc::suseconds_t,
        };
        for opt in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
            let ret = unsafe {
                libc::setsockopt(
                    self.fd,
                    libc::SOL_SOCKET,
                    opt,
                    &tv as *const libc::timeval as *const libc::c_void,
                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                )
            };
            if ret < 0 {
                return Err(Error::Io(std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }

    /// Drain any pending input data.
    fn drain(&self) {
        let mut buf = [0u8; 1024];
        // Set non-blocking temporarily
        unsafe {
            let flags = libc::fcntl(self.fd, libc::F_GETFL);
            libc::fcntl(self.fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            loop {
                let n = libc::recv(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0);
                if n <= 0 {
                    break;
                }
            }
            libc::fcntl(self.fd, libc::F_SETFL, flags);
        }
    }

    /// Write data in chunks with delays (matching BluetoothUtils.ConnectedThread.write).
    pub fn write_chunked(&self, data: &[u8], chunk_size: usize, delay: Duration) -> Result<()> {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk_size).min(data.len());
            self.drain();
            let chunk = &data[offset..end];
            let mut sent = 0;
            while sent < chunk.len() {
                let n = unsafe {
                    libc::send(
                        self.fd,
                        chunk[sent..].as_ptr() as *const libc::c_void,
                        chunk.len() - sent,
                        0,
                    )
                };
                if n < 0 {
                    return Err(Error::Io(std::io::Error::last_os_error()));
                }
                sent += n as usize;
            }
            std::thread::sleep(delay);
            offset = end;
        }
        Ok(())
    }

    /// Read response by polling.
    pub fn read_response(
        &self,
        max_wait: Duration,
        poll_interval: Duration,
    ) -> Result<Option<Vec<u8>>> {
        let mut response = Vec::new();
        let polls = (max_wait.as_millis() / poll_interval.as_millis().max(1)) as usize;
        let mut buf = [0u8; 512];

        // Set short timeout for polling
        let poll_tv = libc::timeval {
            tv_sec: 0,
            tv_usec: poll_interval.as_micros() as libc::suseconds_t,
        };
        unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &poll_tv as *const libc::timeval as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            );
        }

        for _ in 0..polls {
            let n =
                unsafe { libc::recv(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
            if n > 0 {
                response.extend_from_slice(&buf[..n as usize]);
                // Brief extra wait for trailing bytes
                let extra_tv = libc::timeval {
                    tv_sec: 0,
                    tv_usec: 50000, // 50ms
                };
                unsafe {
                    libc::setsockopt(
                        self.fd,
                        libc::SOL_SOCKET,
                        libc::SO_RCVTIMEO,
                        &extra_tv as *const libc::timeval as *const libc::c_void,
                        std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                    );
                }
                let n2 = unsafe {
                    libc::recv(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0)
                };
                if n2 > 0 {
                    response.extend_from_slice(&buf[..n2 as usize]);
                }
                break;
            }
        }

        // Restore timeout
        self.set_timeout(self.timeout)?;

        if response.is_empty() {
            Ok(None)
        } else {
            Ok(Some(response))
        }
    }

    /// Send a 16-byte command and read response.
    pub fn send_cmd(&self, cmd_frame: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        log::debug!("TX: {:02x?}", cmd_frame);
        self.write_chunked(cmd_frame, 512, Duration::from_millis(10))?;
        let resp = self.read_response(Duration::from_secs(2), Duration::from_millis(20))?;
        if let Some(ref data) = resp {
            log::debug!("RX: {:02x?}", &data[..data.len().min(40)]);
        } else {
            log::debug!("RX: (no response)");
        }
        Ok(resp)
    }

    /// Send a 512-byte data frame as 4x128-byte chunks with 10ms delay.
    pub fn send_data_frame(
        &self,
        frame: &[u8; 512],
        read_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        for i in 0..4 {
            let chunk = &frame[i * 128..(i + 1) * 128];
            std::thread::sleep(Duration::from_millis(10));
            self.drain();
            let mut sent = 0;
            while sent < chunk.len() {
                let n = unsafe {
                    libc::send(
                        self.fd,
                        chunk[sent..].as_ptr() as *const libc::c_void,
                        chunk.len() - sent,
                        0,
                    )
                };
                if n < 0 {
                    return Err(Error::Io(std::io::Error::last_os_error()));
                }
                sent += n as usize;
            }
        }
        if read_response {
            self.read_response(Duration::from_secs(2), Duration::from_millis(20))
        } else {
            Ok(None)
        }
    }
}

impl Drop for RfcommSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Parse a Bluetooth address string "XX:XX:XX:XX:XX:XX" into 6 bytes.
/// BlueZ uses reversed byte order (LSB first).
fn parse_bdaddr(addr: &str) -> Result<[u8; 6]> {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 6 {
        return Err(Error::InvalidParam(format!("invalid BT address: {addr}")));
    }
    let mut bdaddr = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bdaddr[5 - i] = u8::from_str_radix(part, 16)
            .map_err(|_| Error::InvalidParam(format!("invalid BT address byte: {part}")))?;
    }
    Ok(bdaddr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bdaddr() {
        let addr = parse_bdaddr("A4:93:40:A0:87:57").unwrap();
        // BlueZ reversed: 57:87:A0:40:93:A4
        assert_eq!(addr, [0x57, 0x87, 0xA0, 0x40, 0x93, 0xA4]);
    }

    #[test]
    fn test_parse_bdaddr_invalid() {
        assert!(parse_bdaddr("not-an-address").is_err());
        assert!(parse_bdaddr("A4:93:40:A0:87").is_err());
        assert!(parse_bdaddr("A4:93:40:A0:87:XX").is_err());
    }
}
