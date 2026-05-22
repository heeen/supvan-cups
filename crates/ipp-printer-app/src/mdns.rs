//! mDNS / DNS-SD advertising for IPP printers (`_ipp._tcp.local.`).
//!
//! Off by default; enable via the `mdns` feature on this crate. When enabled,
//! [`Advertiser::register_all`] publishes one service instance per printer in
//! the registry, with the TXT records CUPS / cups-browsed expect for
//! IPP-Everywhere auto-discovery (RFC 8011 + Bonjour for IPP, PWG 5100.14).

use std::collections::HashMap;

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::printer::PrinterRegistry;

const IPP_SERVICE: &str = "_ipp._tcp.local.";

/// Holds the [`ServiceDaemon`] and the list of registered fullnames so we can
/// unregister cleanly on drop.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullnames: Vec<String>,
}

impl Advertiser {
    /// Start a daemon and register every printer in the registry.
    pub fn register_all(registry: &PrinterRegistry, port: u16) -> mdns_sd::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let host = hostname();
        let mut fullnames = Vec::new();
        for rec in registry.read().iter() {
            let info = service_info(&host, port, &rec.config.name, &rec.config.make_and_model)?;
            let fullname = info.get_fullname().to_string();
            daemon.register(info)?;
            log::info!("mdns: registered {fullname}");
            fullnames.push(fullname);
        }
        Ok(Self { daemon, fullnames })
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        for fullname in &self.fullnames {
            let _ = self.daemon.unregister(fullname);
        }
        let _ = self.daemon.shutdown();
    }
}

fn hostname() -> String {
    let h = std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    // mdns-sd normalises trailing ".local." — pass bare hostname.
    h
}

fn service_info(
    host: &str,
    port: u16,
    name: &str,
    make_and_model: &str,
) -> mdns_sd::Result<ServiceInfo> {
    let mut txt: HashMap<String, String> = HashMap::new();
    txt.insert("rp".into(), format!("ipp/print/{name}"));
    txt.insert("ty".into(), make_and_model.to_string());
    txt.insert("note".into(), make_and_model.to_string());
    txt.insert("product".into(), format!("({make_and_model})"));
    // Document formats CUPS asks for during driverless setup.
    txt.insert(
        "pdl".into(),
        "image/pwg-raster,application/vnd.cups-raster,application/octet-stream".into(),
    );
    // IPP Everywhere advertises URF=…; CUPS reads this for the everywhere driver.
    txt.insert("URF".into(), "W8,SRGB24,CP1,RS203".into());
    txt.insert("Color".into(), "F".into());
    txt.insert("Duplex".into(), "F".into());
    txt.insert("adminurl".into(), format!("http://{host}.local:{port}/"));
    txt.insert("priority".into(), "0".into());
    txt.insert("qtotal".into(), "1".into());
    // TXT version per PWG 5100.14.
    txt.insert("txtvers".into(), "1".into());

    let info = ServiceInfo::new(
        IPP_SERVICE,
        name,
        &format!("{host}.local."),
        "", // IPs filled by enable_addr_auto
        port,
        txt,
    )?
    .enable_addr_auto();
    Ok(info)
}
