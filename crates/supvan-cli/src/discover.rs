//! Scan for printers on every transport.

use crate::CliResult;

/// One line per device, per transport, with the target string to paste into
/// any other subcommand.
pub async fn discover() -> CliResult {
    supvan_discover::models::load();

    eprintln!("Scanning USB, Bluetooth and BLE...");
    let supvan_discover::Candidates { usb, bt, ble } = supvan_discover::scan_all().await;
    eprintln!();

    // (transport, model, serial name, target)
    let mut rows: Vec<(&str, String, String, String)> = Vec::new();
    for u in &usb {
        rows.push((
            "USB",
            u.model_name.clone(),
            u.printer_name.clone().unwrap_or_else(|| "?".into()),
            u.hidraw_path.clone(),
        ));
    }
    for b in &bt {
        rows.push(("BT", model_of(&b.name), b.name.clone(), b.address.clone()));
    }
    for e in &ble {
        rows.push((
            "BLE",
            model_of(&e.name),
            e.name.clone(),
            format!("ble://{}", e.address),
        ));
    }

    if rows.is_empty() {
        eprintln!("No Supvan printers found.");
        eprintln!();
        eprintln!("USB: check the printer is on and the udev rule is installed");
        eprintln!("     (70-supvan-t50.rules — without it /dev/hidraw* is root-only).");
        eprintln!("BT:  the printer must be paired, or in pairing range for the scan.");
        if cfg!(not(feature = "ble")) {
            eprintln!("BLE: not compiled in — rebuild with --features ble.");
        }
        return Ok(());
    }

    // Pad to the widest cell so the targets line up and stay easy to copy.
    let w_model = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    let w_name = rows.iter().map(|r| r.2.len()).max().unwrap_or(0);
    for (transport, model, name, target) in &rows {
        println!("{transport:<4}  {model:<w_model$}  {name:<w_name$}  {target}");
    }

    println!();
    let n = rows.len();
    let plural = if n == 1 { "" } else { "s" };
    println!("{n} device{plural}. Pass a target to any subcommand:");
    println!("  supvan-cli probe {}", rows[0].3);
    Ok(())
}

/// Marketing model behind an advertised serial name, or a placeholder when the
/// registry has no prefix for it — an unlisted hardware code still shows up.
fn model_of(advertised: &str) -> String {
    supvan_discover::models::bt_model_for_name(advertised)
        .unwrap_or("unknown")
        .to_string()
}
