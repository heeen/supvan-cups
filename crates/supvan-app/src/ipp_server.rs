//! Application entry: IPP server, discovery, state.

use std::sync::Arc;

use parking_lot::RwLock;
use ipp_printer_app::{
    default_state_path, DeviceBackend, JobContext, JobFailure, PrinterConfig, PrinterReason,
    PrinterRegistry, Server, ServerOptions,
};

use crate::ipp_job::{config_from_family, run_cups_raster_job};
use crate::models;

pub struct SupvanDeviceBackend;

impl DeviceBackend for SupvanDeviceBackend {
    fn list(&self, emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
        let usb_available = crate::usb_discover::has_device();
        crate::usb_discover::discover(&mut *emit);
        if !usb_available {
            crate::discover::discover(&mut *emit);
        }
    }

    fn poll_status(&self, config: &PrinterConfig) -> Option<PrinterReason> {
        // Open the device just long enough to query status. BT reuses the cache
        // (see device.rs); USB opens a fresh hidraw handle each poll.
        let dev = if config.device_uri.starts_with("btrfcomm://") {
            crate::device::open_bt(&config.device_uri).map(|b| *b)
        } else if config.device_uri.starts_with("usbhid://") {
            crate::device::open_usb(&config.device_uri)
        } else {
            None
        }?;
        Some(dev.status())
    }

    fn driver_for_device(&self, device_id: &str, device_uri: &str) -> Option<String> {
        if !device_id.is_empty() {
            if let Some(mdl) = models::parse_mdl(device_id) {
                let family = models::family_for_model_hint(mdl);
                return Some(family.driver_name.to_string_lossy().into_owned());
            }
        }
        if device_uri.starts_with("usbhid://") {
            return Some(
                models::default_family()
                    .driver_name
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        if device_uri.starts_with("btrfcomm://") {
            return Some(
                models::default_family()
                    .driver_name
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        None
    }
}

pub async fn run_server(host: &str, port: u16) -> std::io::Result<()> {
    models::load();

    let registry: PrinterRegistry = Arc::new(RwLock::new(Vec::new()));
    let state_path = default_state_path("supvan-printer-app");
    let backend = Arc::new(SupvanDeviceBackend);

    Server::bootstrap_printers(
        &registry,
        backend.as_ref(),
        &state_path,
        config_from_family,
    );

    prune_stale_usb(&registry);
    Server::persist(&registry, &state_path);

    let registry_print = registry.clone();
    let print_job = Arc::new(
        move |ctx: JobContext, raster: Vec<u8>, copies: u32| -> Result<(), JobFailure> {
            let cfg = {
                let guard = registry_print.read();
                guard
                    .iter()
                    .find(|p| p.config.name == ctx.printer_name)
                    .ok_or_else(|| {
                        JobFailure::other(format!("printer not found: {}", ctx.printer_name))
                    })?
                    .config
                    .clone()
            };
            run_cups_raster_job(
                &cfg.name,
                &cfg.device_uri,
                cfg.darkness,
                cfg.printhead_width_dots,
                &cfg.driver_name,
                &raster,
                copies,
            )
        },
    );

    Server::run(ServerOptions {
        host: host.to_string(),
        port,
        printers: registry,
        device_backend: backend,
        print_job,
        state_path,
    })
    .await
}

fn prune_stale_usb(registry: &PrinterRegistry) {
    let mut guard = registry.write();
    guard.retain(|p| {
        if let Some(id) = p.config.device_uri.strip_prefix("usbhid://") {
            if crate::usb_discover::find_device_by_id(id).is_none() {
                log::info!("pruning stale USB printer: {}", p.config.device_uri);
                return false;
            }
        }
        true
    });
}
