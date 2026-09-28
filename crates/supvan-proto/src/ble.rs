//! BLE GATT transport for E11/E12-class printers (BLE-only hardware).
//!
//! BLE carries the **same** 16-byte SPP command framing and 512-byte data
//! frames as Classic Bluetooth, so [`BlePipe`] is just another [`SppPipe`] —
//! the shared [`crate::spp_pipe::SppCodec`] drives it unchanged. Only the byte
//! pipe differs: GATT notify/write characteristics instead of an RFCOMM stream.
//!
//! The wire details follow the vendor Android app (`BLEUtils.java`):
//! - Connect (TRANSPORT_LE, no bonding — the app never calls `createBond`),
//!   let BlueZ negotiate the ATT MTU (the app requests 200), discover
//!   services, enable notifications.
//! - One of three service/characteristic patterns is auto-detected (see
//!   [`chars_for_service`]); first match wins.
//! - Every frame goes out as an ATT **write request** (`BLEUtils.write`), in
//!   128-byte fragments: `BasePrint.transferSplitData` splits each 512-byte
//!   data frame into 4 × 128 with ~10 ms between them.
//! - A command notification echoes the request's command byte at offset 7;
//!   poll up to ~4 s for the match (the app loops 200 × 20 ms). Data frames
//!   are **not** acked over BLE — see [`SppPipe::acks_data_frames`].
//!
//! This transport is **unverified against hardware** — we own no BLE printer.
//! It is gated behind the `ble` feature; the pure framing helpers below are
//! always compiled and unit-tested.

use uuid::Uuid;

/// Build a 16-bit-assigned Bluetooth SIG UUID
/// (`0000XXXX-0000-1000-8000-00805f9b34fb`).
const fn sig(short: u16) -> Uuid {
    Uuid::from_u128(0x0000_0000_0000_1000_8000_0080_5f9b_34fb_u128 | ((short as u128) << 96))
}

/// Vendor-custom service base for the `e0ff` family
/// (`0000e0ff-3c17-d293-8e48-14fe2e4da212`).
const E0FF_SERVICE: Uuid = Uuid::from_u128(0x0000_e0ff_3c17_d293_8e48_14fe_2e4d_a212);

/// Given a discovered GATT **service** UUID, return the
/// `(notify_char, write_char)` UUIDs if it's one of the three known Supvan
/// patterns. First match wins, mirroring `BLEUtils.getService`:
/// - `fee7`  → notify == write `fec1`
/// - `e0ff`  → notify `ffe1`, write `ffe9`
/// - `ff00`  → notify `ff01`, write `ff02`
pub fn chars_for_service(service: Uuid) -> Option<(Uuid, Uuid)> {
    if service == E0FF_SERVICE {
        return Some((sig(0xffe1), sig(0xffe9)));
    }
    // Bluetooth-base services match on their 16-bit short code.
    let short = (service.as_u128() >> 96) as u16;
    if service == sig(short) {
        return match short {
            0xfee7 => Some((sig(0xfec1), sig(0xfec1))), // notify and write share one char
            0xff00 => Some((sig(0xff01), sig(0xff02))),
            _ => None,
        };
    }
    None
}

/// A response notification answers a command when it echoes the command byte at
/// offset 7 of the SPP frame (the same offset the request carries it).
#[cfg_attr(not(feature = "ble"), allow(dead_code))]
fn response_matches(resp: &[u8], cmd: u8) -> bool {
    resp.get(7) == Some(&cmd)
}

#[cfg(feature = "ble")]
pub use imp::BlePipe;

#[cfg(feature = "ble")]
mod imp {
    use super::{chars_for_service, response_matches};
    use crate::error::{Error, Result};
    use crate::spp_pipe::SppPipe;
    use bluer::gatt::WriteOp;
    use bluer::gatt::remote::{Characteristic, CharacteristicWriteRequest};
    use bluer::{Adapter, Address, Device, DiscoveryFilter, DiscoveryTransport, Session};
    use futures_util::StreamExt;
    use std::pin::Pin;
    use std::time::Duration;
    use tokio::sync::Mutex;

    /// Response poll budget — matches the vendor app's 200 × 20 ms ≈ 4 s.
    const RESPONSE_TIMEOUT: Duration = Duration::from_secs(4);
    /// GATT write fragment size: the vendor splits every 512-byte data frame
    /// into 4 × 128 bytes, so 128 is what the firmware is fed.
    const BLE_CHUNK: usize = 128;
    /// Pacing between fragments, as passed by the T50/G15 print flows.
    const BLE_CHUNK_DELAY: Duration = Duration::from_millis(10);
    /// BlueZ can leave `Connect()` pending for minutes (bluer's own D-Bus
    /// budget is 120 s). Give up sooner and retry — an LE connect that is
    /// going to work generally works within a few seconds.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
    const CONNECT_ATTEMPTS: usize = 3;
    /// How long to scan for a printer BlueZ has never seen (or has aged out of
    /// its cache) before giving up on the address.
    const DISCOVERY_WINDOW: Duration = Duration::from_secs(8);

    type NotifyStream = Pin<Box<dyn futures_util::Stream<Item = Vec<u8>> + Send>>;

    /// A GATT byte pipe: writes SPP frames to the write characteristic and
    /// reads responses from the notify characteristic.
    pub struct BlePipe {
        write_char: Characteristic,
        notify: Mutex<NotifyStream>,
        // Keep the connection + session alive for the pipe's lifetime.
        _device: Device,
        _session: Session,
    }

    fn map_err(e: bluer::Error) -> Error {
        Error::Ble(e.to_string())
    }

    impl BlePipe {
        /// Connect to a BLE printer by address, discover its Supvan GATT
        /// service, and subscribe to notifications.
        pub async fn connect(address: &str) -> Result<Self> {
            let session = Session::new().await.map_err(map_err)?;
            let adapter = session.default_adapter().await.map_err(map_err)?;
            adapter.set_powered(true).await.map_err(map_err)?;
            let addr: Address = address
                .parse()
                .map_err(|_| Error::InvalidParam(format!("invalid BLE address: {address}")))?;

            // BlueZ can only connect to a device it currently knows about, and
            // it drops unpaired ones from its cache once they stop advertising.
            // Scan first if it has forgotten this one — and finish scanning
            // before connecting, because an open discovery session is the
            // classic reason `Connect()` never returns.
            let known = adapter.device_addresses().await.map_err(map_err)?;
            if !known.contains(&addr) {
                scan_for(&adapter, addr).await?;
            }

            let device = adapter.device(addr).map_err(map_err)?;
            connect_with_retry(&device).await?;
            // Trusted lets BlueZ re-establish the link later without an agent,
            // matching what the classic path does after pairing.
            if let Err(e) = device.set_trusted(true).await {
                log::debug!("BLE: could not set Trusted on {address}: {e}");
            }

            let (notify_char, write_char) = find_chars(&device).await?;
            let notify = notify_char.notify().await.map_err(map_err)?;
            Ok(Self {
                write_char,
                notify: Mutex::new(Box::pin(notify)),
                _device: device,
                _session: session,
            })
        }

        /// Write `data` as paced [`BLE_CHUNK`]-sized ATT write requests.
        ///
        /// `bluer`'s `Characteristic::write` defaults to `WriteOp::Command`
        /// (write *without* response), so the op type has to be spelled out —
        /// the vendor app writes every frame with response.
        async fn write_chunked(&self, data: &[u8]) -> Result<()> {
            let req = CharacteristicWriteRequest {
                op_type: WriteOp::Command,
                ..Default::default()
            };
            for (i, chunk) in data.chunks(BLE_CHUNK).enumerate() {
                if i > 0 {
                    tokio::time::sleep(BLE_CHUNK_DELAY).await;
                }
                self.write_char
                    .write_ext(chunk, &req)
                    .await
                    .map_err(map_err)?;
            }
            Ok(())
        }

        /// Wait up to [`RESPONSE_TIMEOUT`] for a notification echoing `cmd` at
        /// offset 7, discarding anything else that arrives meanwhile.
        async fn await_response(&self, cmd: u8) -> Result<Option<Vec<u8>>> {
            let mut stream = self.notify.lock().await;
            let matching = async {
                while let Some(payload) = stream.next().await {
                    if response_matches(&payload, cmd) {
                        return Some(payload);
                    }
                }
                None
            };
            Ok(tokio::time::timeout(RESPONSE_TIMEOUT, matching)
                .await
                .unwrap_or(None))
        }
    }

    /// Run an LE scan until `addr` shows up, then stop it.
    async fn scan_for(adapter: &Adapter, addr: Address) -> Result<()> {
        log::info!("BLE: {addr} not known to BlueZ, scanning");
        adapter
            .set_discovery_filter(DiscoveryFilter {
                transport: DiscoveryTransport::Le,
                ..Default::default()
            })
            .await
            .map_err(map_err)?;
        let mut events = adapter.discover_devices().await.map_err(map_err)?;
        let found = async {
            while let Some(ev) = events.next().await {
                if matches!(ev, bluer::AdapterEvent::DeviceAdded(a) if a == addr) {
                    return true;
                }
            }
            false
        };
        let found = tokio::time::timeout(DISCOVERY_WINDOW, found)
            .await
            .unwrap_or(false);
        // Dropping the stream ends the discovery session; do it before the
        // caller connects.
        drop(events);
        if !found {
            return Err(Error::Ble(format!("{addr} did not advertise")));
        }
        Ok(())
    }

    /// Connect, bounding each attempt well inside bluer's 120 s D-Bus budget.
    async fn connect_with_retry(device: &Device) -> Result<()> {
        let mut last = None;
        for attempt in 1..=CONNECT_ATTEMPTS {
            if device.is_connected().await.map_err(map_err)? {
                return Ok(());
            }
            match tokio::time::timeout(CONNECT_TIMEOUT, device.connect()).await {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(e)) => {
                    log::warn!("BLE: connect attempt {attempt} failed: {e}");
                    last = Some(e.to_string());
                }
                Err(_) => {
                    log::warn!("BLE: connect attempt {attempt} timed out");
                    last = Some(format!("no reply from BlueZ within {CONNECT_TIMEOUT:?}"));
                    // A stuck Connect stays pending inside BlueZ; Disconnect is
                    // the documented way to cancel it before trying again.
                    let _ = device.disconnect().await;
                }
            }
        }
        Err(Error::Ble(last.unwrap_or_else(|| "connect failed".into())))
    }

    async fn find_chars(device: &Device) -> Result<(Characteristic, Characteristic)> {
        for service in device.services().await.map_err(map_err)? {
            let su = service.uuid().await.map_err(map_err)?;
            let Some((notify_uuid, write_uuid)) = chars_for_service(su) else {
                continue;
            };
            let (mut notify_char, mut write_char) = (None, None);
            for c in service.characteristics().await.map_err(map_err)? {
                let cu = c.uuid().await.map_err(map_err)?;
                if cu == notify_uuid {
                    notify_char = Some(c.clone());
                }
                if cu == write_uuid {
                    write_char = Some(c);
                }
            }
            if let (Some(n), Some(w)) = (notify_char, write_char) {
                log::info!("BLE: service {su} (notify {notify_uuid}, write {write_uuid})");
                return Ok((n, w));
            }
        }
        Err(Error::Ble("no supported Supvan GATT service found".into()))
    }

    #[async_trait::async_trait]
    impl SppPipe for BlePipe {
        async fn send_cmd_frame(&self, frame: &[u8; 16]) -> Result<Option<Vec<u8>>> {
            self.write_chunked(frame).await?;
            self.await_response(frame[7]).await
        }

        /// The flag is moot here: [`Self::acks_data_frames`] is false, so the
        /// codec never asks for a reply that BLE would not send.
        async fn send_data_frame(
            &self,
            frame: &[u8; 512],
            _read_response: bool,
        ) -> Result<Option<Vec<u8>>> {
            self.write_chunked(frame).await?;
            Ok(None)
        }

        fn acks_data_frames(&self) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee7_service_maps_to_shared_fec1_char() {
        let svc = Uuid::parse_str("0000fee7-0000-1000-8000-00805f9b34fb").unwrap();
        let fec1 = Uuid::parse_str("0000fec1-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(chars_for_service(svc), Some((fec1, fec1)));
    }

    #[test]
    fn e0ff_service_maps_to_ffe1_ffe9() {
        let svc = Uuid::parse_str("0000e0ff-3c17-d293-8e48-14fe2e4da212").unwrap();
        let notify = Uuid::parse_str("0000ffe1-0000-1000-8000-00805f9b34fb").unwrap();
        let write = Uuid::parse_str("0000ffe9-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(chars_for_service(svc), Some((notify, write)));
    }

    #[test]
    fn ff00_service_maps_to_ff01_ff02() {
        let svc = Uuid::parse_str("0000ff00-0000-1000-8000-00805f9b34fb").unwrap();
        let notify = Uuid::parse_str("0000ff01-0000-1000-8000-00805f9b34fb").unwrap();
        let write = Uuid::parse_str("0000ff02-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(chars_for_service(svc), Some((notify, write)));
    }

    #[test]
    fn unknown_service_is_rejected() {
        // Generic Access (0x1800) is not one of ours.
        let svc = Uuid::parse_str("00001800-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(chars_for_service(svc), None);
        // A 16-bit short code on a non-Bluetooth base must not match.
        let fake = Uuid::parse_str("0000fee7-1234-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(chars_for_service(fake), None);
    }

    #[test]
    fn response_matches_command_echo_at_offset_7() {
        // 16-byte command frame: cmd byte sits at index 7.
        let cmd = 0x12;
        let mut resp = vec![0u8; 16];
        resp[7] = cmd;
        assert!(response_matches(&resp, cmd));
        resp[7] = 0x99;
        assert!(!response_matches(&resp, cmd));
        // Too-short payloads never match.
        assert!(!response_matches(&[0x7e, 0x5a], cmd));
    }
}
