//! Raw Bluetooth RFCOMM socket using libc.
//!
//! Port of the Android app's `BluetoothUtils.ConnectedThread`: drain stale
//! input before every write, pace writes, poll for the reply.

use crate::cmd::{CmdFrame, frame_len};
use crate::data::DataFrame;
use crate::error::{Error, Result};
use crate::spp_pipe::{LinkCost, SPP_CHUNK, SPP_CHUNK_DELAY};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

// Bluetooth socket constants (from <bluetooth/bluetooth.h> and <bluetooth/rfcomm.h>)
const AF_BLUETOOTH: i32 = 31;
const BTPROTO_RFCOMM: i32 = 3;
const RFCOMM_CHANNEL: u8 = 1;

/// Socket send/receive timeout outside a reply poll, and the reply budget.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// How often a reply poll wakes to check for bytes (the vendor's 20 ms).
const REPLY_POLL: Duration = Duration::from_millis(20);
/// Grace window for stragglers of a reply that isn't a `7E 5A` frame.
const UNFRAMED_TRAILING_WAIT: Duration = Duration::from_millis(50);

/// Measured on a T50M Pro: a data frame costs ~50 ms (four paced chunks plus
/// the ack round trip), a block's three commands ~180 ms. A dense 10.5 mm
/// buffer (~2.5 KB, five frames) therefore takes ~0.43 s — slower than the
/// printhead at the speed `calc_speed` picks for it.
const LINK_COST: LinkCost = LinkCost {
    per_frame: Duration::from_millis(50),
    per_block: Duration::from_millis(180),
};

/// sockaddr_rc structure for RFCOMM connections.
#[repr(C)]
struct SockaddrRc {
    rc_family: u16,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

/// A Bluetooth RFCOMM socket connection to a printer.
pub struct RfcommSocket {
    fd: OwnedFd,
}

impl RfcommSocket {
    /// Connect to a Bluetooth device by address string (e.g. "A4:93:40:A0:87:57").
    pub fn connect(addr: &str) -> Result<Self> {
        let bdaddr = parse_bdaddr(addr)?;

        let raw = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
        if raw < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        // SAFETY: `raw` is a freshly opened socket nothing else owns.
        let sock = Self {
            fd: unsafe { OwnedFd::from_raw_fd(raw) },
        };

        let sa = SockaddrRc {
            rc_family: AF_BLUETOOTH as u16,
            rc_bdaddr: bdaddr,
            rc_channel: RFCOMM_CHANNEL,
        };
        let ret = unsafe {
            libc::connect(
                sock.raw(),
                &sa as *const SockaddrRc as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }

        sock.set_timeout(libc::SO_RCVTIMEO, IO_TIMEOUT)?;
        sock.set_timeout(libc::SO_SNDTIMEO, IO_TIMEOUT)?;
        Ok(sock)
    }

    fn raw(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    fn set_timeout(&self, opt: i32, timeout: Duration) -> Result<()> {
        let tv = libc::timeval {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_usec: timeout.subsec_micros() as libc::suseconds_t,
        };
        let ret = unsafe {
            libc::setsockopt(
                self.raw(),
                libc::SOL_SOCKET,
                opt,
                &tv as *const libc::timeval as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// One `recv`; `None` on timeout, would-block, error or EOF.
    fn recv(&self, buf: &mut [u8], flags: i32) -> Option<usize> {
        let n = unsafe { libc::recv(self.raw(), buf.as_mut_ptr().cast(), buf.len(), flags) };
        usize::try_from(n).ok().filter(|&n| n > 0)
    }

    /// Discard anything already received — a stale reply would otherwise be
    /// read as the answer to the next command.
    fn drain(&self) {
        let mut buf = [0u8; 1024];
        while self.recv(&mut buf, libc::MSG_DONTWAIT).is_some() {}
    }

    fn send_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let n = unsafe { libc::send(self.raw(), data.as_ptr().cast(), data.len(), 0) };
            let Ok(n) = usize::try_from(n) else {
                return Err(Error::Io(std::io::Error::last_os_error()));
            };
            data = &data[n..];
        }
        Ok(())
    }

    /// Poll for a reply. Returns as soon as a `7E 5A` frame is complete — a
    /// fixed grace wait here cost ~50 ms per data-packet ack, enough to starve
    /// the printhead mid-label over Bluetooth.
    fn read_reply(&self) -> Result<Option<Vec<u8>>> {
        self.set_timeout(libc::SO_RCVTIMEO, REPLY_POLL)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 512];
        let polls = IO_TIMEOUT.as_millis() / REPLY_POLL.as_millis();
        for _ in 0..polls {
            let Some(n) = self.recv(&mut buf, 0) else {
                if !response.is_empty() && frame_len(&response).is_none() {
                    break;
                }
                continue;
            };
            response.extend_from_slice(&buf[..n]);
            match frame_len(&response) {
                Some(len) if response.len() >= len => break,
                Some(_) => {}
                None if response.len() < 4 => {}
                None => {
                    self.set_timeout(libc::SO_RCVTIMEO, UNFRAMED_TRAILING_WAIT)?;
                    if let Some(n) = self.recv(&mut buf, 0) {
                        response.extend_from_slice(&buf[..n]);
                    }
                    break;
                }
            }
        }
        self.set_timeout(libc::SO_RCVTIMEO, IO_TIMEOUT)?;
        Ok((!response.is_empty()).then_some(response))
    }

    /// Send a command frame and read the reply.
    fn send_cmd_blocking(&self, frame: &CmdFrame) -> Result<Option<Vec<u8>>> {
        log::debug!("TX: {frame:02x?}");
        self.drain();
        self.send_all(frame)?;
        std::thread::sleep(SPP_CHUNK_DELAY);
        let resp = self.read_reply()?;
        match &resp {
            Some(data) => log::debug!("RX: {:02x?}", &data[..data.len().min(40)]),
            None => log::debug!("RX: (no response)"),
        }
        Ok(resp)
    }

    /// Send a data frame as paced 128-byte chunks (`transferSplitData`).
    fn send_data_frame_blocking(
        &self,
        frame: &DataFrame,
        read_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        for chunk in frame.chunks(SPP_CHUNK) {
            std::thread::sleep(SPP_CHUNK_DELAY);
            self.drain();
            self.send_all(chunk)?;
        }
        if read_response {
            self.read_reply()
        } else {
            Ok(None)
        }
    }
}

#[async_trait::async_trait]
impl crate::spp_pipe::SppPipe for RfcommSocket {
    async fn send_cmd_frame(&self, frame: &CmdFrame) -> Result<Option<Vec<u8>>> {
        // Blocking libc socket I/O: run in place so concurrent tasks (status
        // polling, IPP serving) migrate off this worker during the round-trip.
        tokio::task::block_in_place(|| self.send_cmd_blocking(frame))
    }

    async fn send_data_frame(
        &self,
        frame: &DataFrame,
        read_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        tokio::task::block_in_place(|| self.send_data_frame_blocking(frame, read_response))
    }

    fn link_cost(&self) -> Option<LinkCost> {
        Some(LINK_COST)
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
