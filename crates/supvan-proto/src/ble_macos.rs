//! macOS GATT byte pipe: CoreBluetooth via `btleplug`.
//!
//! Mirrors the Linux `bluer` pipe in `ble.rs` step for step — connect, discover
//! the Supvan service, subscribe to notify, write every frame as a paced ATT
//! write **request**, poll for the command echo — because both are the same
//! vendor protocol over the same ATT operations. Only the addressing differs:
//! CoreBluetooth hides the MAC and identifies a peripheral by a per-machine
//! `NSUUID`, so [`BlePipe::connect`] takes the `PeripheralId` string that
//! discovery prints, not an `AA:BB:CC:DD:EE:FF`.

use super::{att_chunk, chars_for_service, response_matches};
use crate::cmd::{CMD_OFFSET, CmdFrame};
use crate::data::DataFrame;
use crate::error::{Error, Result};
use crate::spp_pipe::{SPP_CHUNK_DELAY, SppPipe};
use btleplug::api::{
    Central, CentralState, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
    ScanFilter, ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Response poll budget — matches the vendor app's 200 × 20 ms ≈ 4 s.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(4);
/// A `connect()` that is going to work generally answers within a few seconds;
/// CoreBluetooth will otherwise sit on the request until the link supervisor
/// gives up, which is far too long for a print job to wait.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to scan for a peripheral CoreBluetooth has not seen advertise.
const DISCOVERY_WINDOW: Duration = Duration::from_secs(8);

type NotifyStream = Pin<Box<dyn futures_util::Stream<Item = ValueNotification> + Send>>;

/// A GATT byte pipe: writes SPP frames to the write characteristic and reads
/// responses from the notify characteristic.
pub(crate) struct BlePipe {
    write_char: Characteristic,
    notify_uuid: Uuid,
    notify: Mutex<NotifyStream>,
    /// CoreBluetooth sends one ATT value per `write`; keep each chunk inside
    /// the negotiated MTU rather than assuming the vendor's 128-byte fragment.
    chunk: usize,
    // Holding the peripheral keeps the CoreBluetooth link alive for this pipe.
    peripheral: Peripheral,
}

impl BlePipe {
    /// Connect to a BLE printer by CoreBluetooth `PeripheralId`, discover its
    /// Supvan GATT service, and subscribe to notifications.
    pub(crate) async fn connect(address: &str) -> Result<Self> {
        let manager = Manager::new()
            .await
            .map_err(|e| setup("open CoreBluetooth", e))?;
        let adapter = manager
            .adapters()
            .await
            .map_err(|e| setup("list Bluetooth adapters", e))?
            .into_iter()
            .next()
            .ok_or_else(|| Error::BleSetup(format!("no Bluetooth adapter for {address}")))?;
        // CoreBluetooth reports an unauthorized adapter as `Unknown`; without
        // this check the scan simply yields nothing and looks like a missing
        // printer.
        match adapter
            .adapter_state()
            .await
            .map_err(|e| setup("query Bluetooth state", e))?
        {
            CentralState::PoweredOn => {}
            state => {
                return Err(Error::BleSetup(format!(
                    "Bluetooth unavailable ({state:?}); check power and Privacy & Security -> Bluetooth"
                )));
            }
        }

        let peripheral = find_peripheral(&adapter, address).await?;
        if !peripheral
            .is_connected()
            .await
            .map_err(|e| setup("query connection state", e))?
            && let Err(e) = peripheral.connect_with_timeout(CONNECT_TIMEOUT).await
        {
            // The objc connect can still complete after the timeout; drop it
            // so it does not occupy the printer's single LE link.
            let _ = peripheral.disconnect().await;
            return Err(setup("connect", e));
        }
        if let Err(e) = peripheral
            .discover_services_with_timeout(CONNECT_TIMEOUT)
            .await
        {
            let _ = peripheral.disconnect().await;
            return Err(setup("discover GATT services", e));
        }

        let (notify_char, write_char) = find_chars(&peripheral)?;
        peripheral
            .subscribe(&notify_char)
            .await
            .map_err(|e| setup("subscribe to notifications", e))?;
        let notify = peripheral
            .notifications()
            .await
            .map_err(|e| setup("open notification stream", e))?;
        // `mtu()` is the ATT MTU including the 3-byte write header; fall back
        // to the vendor chunk if the stack has not reported one yet.
        let mtu = peripheral.mtu();
        let chunk = att_chunk(mtu);
        log::info!("BLE: MTU {mtu}, writing {chunk}-byte fragments");

        Ok(Self {
            write_char,
            notify_uuid: notify_char.uuid,
            notify: Mutex::new(notify),
            chunk,
            peripheral,
        })
    }

    /// Write `data` as paced ATT write requests, chunked to the negotiated MTU
    /// and capped at the vendor's 128-byte fragment.
    ///
    /// The vendor app writes every frame as a write *request* and only queues
    /// the next fragment once the peer has confirmed the last; without the
    /// response the head outruns the printer's buffer.
    async fn write_chunked(&self, data: &[u8]) -> Result<()> {
        for (i, chunk) in data.chunks(self.chunk).enumerate() {
            if i > 0 {
                tokio::time::sleep(SPP_CHUNK_DELAY).await;
            }
            self.peripheral
                .write(&self.write_char, chunk, WriteType::WithResponse)
                .await
                .map_err(|e| setup("write characteristic", e))?;
        }
        Ok(())
    }

    /// Wait up to [`RESPONSE_TIMEOUT`] for a notification echoing `cmd` at
    /// offset 7, discarding anything else that arrives meanwhile.
    async fn await_response(&self, cmd: u8) -> Result<Option<Vec<u8>>> {
        let mut stream = self.notify.lock().await;
        let matching = async {
            while let Some(notification) = stream.next().await {
                // The stream carries every subscribed characteristic; only the
                // notify one speaks SPP.
                if notification.uuid != self.notify_uuid {
                    continue;
                }
                if response_matches(&notification.value, cmd) {
                    return Some(notification.value);
                }
            }
            None
        };
        Ok(tokio::time::timeout(RESPONSE_TIMEOUT, matching)
            .await
            .unwrap_or(None))
    }
}

/// Find the peripheral CoreBluetooth already knows, else scan for it.
async fn find_peripheral(adapter: &Adapter, address: &str) -> Result<Peripheral> {
    if let Some(peripheral) = cached(adapter, address).await? {
        return Ok(peripheral);
    }
    log::info!("BLE: {address} not cached, scanning");
    // Subscribe to events before starting the scan: CoreBluetooth only replays
    // discoveries that arrive while a subscriber is listening.
    let mut events = adapter
        .events()
        .await
        .map_err(|e| setup("open adapter event stream", e))?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| setup("start scan", e))?;
    let found = async {
        while let Some(event) = events.next().await {
            let id = match event {
                btleplug::api::CentralEvent::DeviceDiscovered(id)
                | btleplug::api::CentralEvent::DeviceUpdated(id) => id,
                _ => continue,
            };
            // The id is the same `NSUUID` string discovery reports.
            if id.to_string() == address {
                return true;
            }
        }
        false
    };
    let found = tokio::time::timeout(DISCOVERY_WINDOW, found)
        .await
        .unwrap_or(false);
    if let Err(e) = adapter.stop_scan().await {
        log::warn!("BLE: could not stop scan: {e}");
    }
    // An open scan competes with the connection attempt; end it first.
    drop(events);
    if !found {
        return Err(Error::BleSetup(format!("{address} did not advertise")));
    }
    cached(adapter, address)
        .await?
        .ok_or_else(|| Error::BleSetup(format!("{address} vanished after scanning")))
}

/// The already-discovered peripheral whose `PeripheralId` matches `address`.
async fn cached(adapter: &Adapter, address: &str) -> Result<Option<Peripheral>> {
    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| setup("list known peripherals", e))?;
    Ok(peripherals
        .into_iter()
        .find(|p| p.id().to_string() == address))
}

fn find_chars(peripheral: &Peripheral) -> Result<(Characteristic, Characteristic)> {
    for service in peripheral.services() {
        let Some((notify_uuid, write_uuid)) = chars_for_service(service.uuid) else {
            continue;
        };
        let (mut notify_char, mut write_char) = (None, None);
        for c in &service.characteristics {
            if c.uuid == notify_uuid {
                notify_char = Some(c.clone());
            }
            // `fee7` puts both on one characteristic, so clone rather than move.
            if c.uuid == write_uuid {
                write_char = Some(c.clone());
            }
        }
        if let (Some(n), Some(w)) = (notify_char, write_char) {
            if !n
                .properties
                .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
            {
                return Err(Error::BleSetup(format!(
                    "notify characteristic {notify_uuid} does not notify"
                )));
            }
            if !w.properties.contains(CharPropFlags::WRITE) {
                return Err(Error::BleSetup(format!(
                    "write characteristic {write_uuid} is not writable with response"
                )));
            }
            let su = service.uuid;
            log::info!("BLE: service {su} (notify {notify_uuid}, write {write_uuid})");
            return Ok((n, w));
        }
    }
    Err(Error::BleSetup(
        "no supported Supvan GATT service found".into(),
    ))
}

/// `btleplug`'s error has no `From` here: name the step that failed so the
/// operator can tell a dead scanner from a refused write.
fn setup(step: &str, e: btleplug::Error) -> Error {
    Error::BleSetup(format!("{step}: {e}"))
}

#[async_trait::async_trait]
impl SppPipe for BlePipe {
    async fn send_cmd_frame(&self, frame: &CmdFrame) -> Result<Option<Vec<u8>>> {
        self.write_chunked(frame).await?;
        self.await_response(frame[CMD_OFFSET]).await
    }

    /// The flag is moot here: [`Self::acks_data_frames`] is false, so the
    /// codec never asks for a reply that BLE would not send.
    async fn send_data_frame(
        &self,
        frame: &DataFrame,
        _read_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        self.write_chunked(frame).await?;
        Ok(None)
    }

    fn acks_data_frames(&self) -> bool {
        false
    }
}
