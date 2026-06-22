//! CUPS queue registration + DNS-SD advertising, coordinated so that the
//! advertised `UUID=` matches a real local queue.
//!
//! Why this ordering matters: a co-resident `cups-browsed` will otherwise
//! auto-create a broken `implicitclass://` queue for our mDNS advert that
//! can't route to a same-host service. `cups-browsed` stands down only if the
//! advert's `UUID=` TXT key matches an existing local CUPS queue's
//! `printer-uuid` (it's the same dedup CUPS's own shared queues rely on).
//! CUPS does NOT adopt our device UUID — the queue gets its own — so we must:
//!
//!   1. create the direct `ipp://localhost:PORT/...` queue,
//!   2. read back the queue's CUPS-assigned `printer-uuid`,
//!   3. stamp it onto the registry record,
//!   4. and only THEN advertise (with that UUID).
//!
//! Because we never advertise until the queue + matching UUID exist,
//! cups-browsed's first sighting already dedupes — no broken queue is ever
//! created. With cups-browsed disabled, the direct queue simply serves. Works
//! either way.

use std::process::Command;
use std::time::Duration;

use ipp_printer_app::PrinterRegistry;

/// Spawn the registrar on a dedicated OS thread. It waits for the IPP server
/// to come up, reconciles the CUPS queue for each printer, stamps each
/// record's UUID from the queue, then advertises and parks holding the
/// advertiser for the process lifetime.
pub fn spawn(registry: PrinterRegistry, port: u16) {
    std::thread::Builder::new()
        .name("cups-registrar".into())
        .spawn(move || run(registry, port))
        .expect("spawn registrar thread");
}

fn run(registry: PrinterRegistry, port: u16) {
    if !wait_for_server(port) {
        log::error!("registrar: IPP server never came up on :{port}; giving up");
        return;
    }

    // Snapshot the queue names to reconcile (release the read lock before
    // shelling out to lpadmin / ipptool).
    let names: Vec<String> = registry.read().iter().map(|r| r.config.name.clone()).collect();

    for name in &names {
        ensure_direct_queue(name, port);
        match read_queue_uuid(name) {
            Some(uuid) => {
                let mut guard = registry.write();
                if let Some(rec) = guard.iter_mut().find(|r| r.config.name == *name) {
                    log::info!("registrar: {name} -> queue printer-uuid {uuid}");
                    rec.uuid = uuid;
                }
            }
            None => log::warn!("registrar: could not read printer-uuid for queue {name}"),
        }
    }

    // Sweep orphans: queues from a previous run that point at our IPP server on
    // this port but whose printer name is no longer discovered — e.g. a
    // model-detection slug change (`…t50m-pro…` → `…t50-series…`), or a queue a
    // crash / manual run left behind. Keep the current printers' queues.
    remove_queues(port, &names);

    // Advertise now that every record carries its queue's UUID. Leak the
    // handle: the advertiser must outlive this function for the daemon's
    // lifetime, and a daemon exit lets the mDNS TTL expire the records.
    // (ipp-printer-app is always built with its default `mdns` feature here.)
    match ipp_printer_app::mdns::Advertiser::register_all(&registry, port) {
        Ok(adv) => {
            log::info!("registrar: mDNS advertising started with queue UUIDs");
            std::mem::forget(adv);
        }
        Err(e) => log::warn!("registrar: mDNS advertise failed: {e}"),
    }

    // Close the startup race: an already-running cups-browsed may have
    // resolved our advert before its local-queue cache caught up with the
    // queue we just made, and created a broken implicitclass duplicate. Now
    // that the queue + matching UUID advert are both live, cups-browsed's
    // subsequent resolves hit its "from local CUPS, ignored" path and won't
    // recreate it — so a few delayed sweeps permanently clear the duplicate.
    for _ in 0..4 {
        std::thread::sleep(Duration::from_secs(5));
        for name in &names {
            let alt = name.replace('-', "_");
            for candidate in [name.clone(), alt] {
                if candidate != *name && queue_is_implicitclass(&candidate) {
                    log::info!("registrar: sweeping racy implicitclass queue {candidate}");
                    let _ = Command::new("lpadmin").args(["-x", &candidate]).status();
                }
            }
        }
    }
}

/// Poll the IPP index until it accepts a connection (server bound). Returns
/// false if it doesn't come up within the budget.
fn wait_for_server(port: u16) -> bool {
    use std::net::TcpStream;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Reconcile the CUPS queue named `name` to a direct `ipp://` device-uri.
/// Removes any cups-browsed `implicitclass://` duplicate first.
fn ensure_direct_queue(name: &str, port: u16) {
    // cups-browsed sanitises the DNS-SD instance name (our hyphenated name)
    // into an underscore queue name. Remove that broken duplicate if present.
    let alt = name.replace('-', "_");
    for candidate in [name.to_string(), alt] {
        if queue_is_implicitclass(&candidate) {
            log::info!("registrar: removing implicitclass queue {candidate}");
            let _ = Command::new("lpadmin").args(["-x", &candidate]).status();
        }
    }

    let uri = format!("ipp://localhost:{port}/ipp/print/{name}");
    let status = Command::new("lpadmin")
        .args(["-p", name, "-E", "-v", &uri, "-m", "everywhere"])
        .status();
    match status {
        Ok(s) if s.success() => log::info!("registrar: ensured direct queue {name} -> {uri}"),
        Ok(s) => log::warn!("registrar: lpadmin for {name} exited {s}"),
        Err(e) => log::warn!("registrar: lpadmin for {name} failed to run: {e}"),
    }
}

/// CUPS queue names whose device-uri targets our IPP server on `port`
/// (`ipp://localhost:<port>/ipp/print/...`).
fn our_queue_names(port: u16) -> Vec<String> {
    let needle = format!("localhost:{port}/ipp/print/");
    let Ok(out) = Command::new("lpstat").arg("-v").output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(&needle))
        // "device for <name>: ipp://localhost:<port>/ipp/print/<name>"
        .filter_map(|l| l.strip_prefix("device for ")?.split(':').next())
        .map(|n| n.trim().to_string())
        .collect()
}

/// Remove every CUPS queue pointing at our IPP server on `port` whose name is
/// not in `keep` — the startup orphan sweep. `keep` is the set of currently
/// discovered printers, whose persistent queues we leave in place (preserving
/// their stable `printer-uuid`). This clears leftovers from a name change or a
/// cups-browsed duplicate without disturbing the live queues.
fn remove_queues(port: u16, keep: &[String]) {
    for q in our_queue_names(port) {
        if keep.iter().any(|k| k == &q) {
            continue;
        }
        log::info!("registrar: removing stale queue {q}");
        let _ = Command::new("lpadmin").args(["-x", &q]).status();
    }
}

/// True if the named queue exists and its device-uri is an implicitclass URI.
fn queue_is_implicitclass(name: &str) -> bool {
    Command::new("lpstat")
        .args(["-v", name])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("implicitclass:"))
        .unwrap_or(false)
}

/// Read the CUPS queue's `printer-uuid` via ipptool, stripping `urn:uuid:`.
fn read_queue_uuid(name: &str) -> Option<String> {
    let test = "/tmp/supvan-gpa.test";
    std::fs::write(
        test,
        "{\nOPERATION Get-Printer-Attributes\n\
         GROUP operation-attributes-tag\n\
         ATTR charset attributes-charset utf-8\n\
         ATTR naturalLanguage attributes-natural-language en\n\
         ATTR uri printer-uri $uri\n\
         STATUS successful-ok\n}\n",
    )
    .ok()?;
    let uri = format!("ipp://localhost:631/printers/{name}");
    let out = Command::new("ipptool").args(["-tv", &uri, test]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // Line looks like: "        printer-uuid (uri) = urn:uuid:XXXX"
    let line = text.lines().find(|l| l.contains("printer-uuid"))?;
    let raw = line.split('=').nth(1)?.trim();
    Some(raw.strip_prefix("urn:uuid:").unwrap_or(raw).to_string())
}
