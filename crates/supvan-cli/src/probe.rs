//! Read-only inspection: device, status and material, plus opcode probing.

use supvan_proto::cmd;
use supvan_proto::printer::Printer;
use supvan_proto::status::{MaterialInfo, PrinterStatus};

use crate::CliResult;

/// Opcodes from the middle of the largest unallocated hole. Used only to learn
/// what "not implemented" looks like on the wire — USB HID does not echo the
/// command byte, so a non-empty response proves nothing on its own and the
/// reply *content* is the only discriminator.
const UNALLOCATED: [u8; 2] = [0x50, 0x51];

/// Trailing bytes of every reply are device-constant; only the head varies.
const COMPARE_LEN: usize = 16;

/// Every status flag, plus the raw registers.
///
/// The firmware sets bits we have no name for, so the registers are shown as
/// well and any undecoded bit is called out — a print that stops with all the
/// known flags clear is precisely when those matter.
fn print_status(s: &PrinterStatus) {
    for (name, value) in s.flags() {
        eprintln!("  {name:<16} {value}");
    }
    eprintln!("  {:<16} {}", "print_count", s.print_count);

    const REG_NAMES: [&str; 4] = ["MSTA lo", "MSTA hi", "FSTA lo", "FSTA hi"];
    eprintln!("  registers:");
    for (i, (name, raw)) in REG_NAMES.iter().zip(s.regs).enumerate() {
        let unknown = raw & !supvan_proto::status::DECODED_BITS[i];
        let note = if unknown != 0 {
            format!("  <- undecoded bits {unknown:#010b}")
        } else {
            String::new()
        };
        eprintln!("    {name}  {raw:#04x}  {raw:08b}{note}");
    }
    if let Some(errs) = s.error_description() {
        eprintln!("  ERRORS: {errs}");
    }
}

/// The loaded roll, one field per line.
fn print_material(m: &MaterialInfo) {
    println!(
        "Label:     {}mm x {}mm  (type={}, gap={}mm)",
        m.width_mm, m.height_mm, m.label_type, m.gap_mm
    );
    println!("Label SN:  {}", m.sn);
    println!("RFID UID:  {}", m.uuid);
    println!("RFID code: {}", m.code);
    match m.remaining {
        Some(r) => println!("Remaining: {r} labels"),
        None => println!("Remaining: (not reported)"),
    }
    match &m.device_sn {
        Some(s) => println!("Device SN: {s}"),
        None => println!("Device SN: (not in this response)"),
    }
}

pub async fn probe(printer: &Printer) -> CliResult {
    if !printer.check_device().await? {
        return Err("device check: no response".into());
    }
    eprintln!("Device: OK");

    if let Some(status) = printer.query_status().await? {
        eprintln!("Status:");
        print_status(&status);
    }
    if let Some(name) = printer.read_device_name().await? {
        eprintln!("Device name: {name}");
    }
    if let Some(fw) = printer.read_firmware_version().await? {
        eprintln!("Firmware:    {fw}");
    }
    if let Some(ver) = printer.read_version().await? {
        eprintln!("Protocol:    {ver}");
    }
    if let Some(mat) = printer.query_material().await? {
        print_material(&mat);
    }
    Ok(())
}

pub async fn material(printer: &Printer) -> CliResult {
    if !printer.check_device().await? {
        return Err("device not responding".into());
    }
    let mat = printer
        .query_material()
        .await?
        .ok_or("no material info (label not installed?)")?;
    print_material(&mat);
    Ok(())
}

/// Opcodes that write, move paper, or enter firmware update. Refused by
/// `probe-reads` regardless of what the caller asks for: the whole point of that
/// command is that it cannot change device state.
fn is_probe_safe(op: u8) -> bool {
    // Firmware update territory — an unrecognised opcode here can leave the unit
    // in a bootloader waiting for an image.
    if (0xC0..=0xEF).contains(&op) {
        return false;
    }
    !matches!(
        op,
        cmd::CMD_ADJ_RESTORE_FACTORY
            | cmd::CMD_ADJ_WRITE_DATA
            | cmd::CMD_ADJ_WRITE_START
            | cmd::CMD_WR_DEV_OPT
            | cmd::BLTCMD_WR_DEV_PAR
            | cmd::CMD_SET_RFID_DATA
            | cmd::CMD_PAPER_BACK
            | cmd::CMD_PAPER_SKIP
            | cmd::CMD_START_PRINT
            | cmd::CMD_SET_TIMESTAMP
            | cmd::CMD_SET_OPTLEVEL
            | cmd::CMD_SET_LAB_YINWEI
            | cmd::CMD_SET_HD_YINWEI
            | cmd::CMD_SET_RL_YINWEI
            | cmd::CMD_SET_TB_YINWEI
            | cmd::CMD_SET_BLTCONTROL
            | cmd::CMD_SET_POWER_OFF_TIME
            | cmd::CMD_SET_BUZZER_KEY
            | cmd::CMD_SET_PRTMODE
            | cmd::BLTCMD_HTIME_SET
    )
}

pub fn parse_opcode(s: &str) -> Result<u8, String> {
    let parsed = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .map(|hex| u8::from_str_radix(hex, 16))
        .unwrap_or_else(|| s.parse());
    let op = parsed.map_err(|_| format!("bad opcode `{s}`"))?;
    if is_probe_safe(op) {
        Ok(op)
    } else {
        Err(format!(
            "0x{op:02X} writes, moves paper, or is in the firmware range"
        ))
    }
}

pub async fn probe_reads(printer: &Printer, also: Vec<u8>) -> CliResult {
    /// Read-only opcodes worth asking about, with what we expect them to mean.
    const READS: &[(u8, &str)] = &[
        (cmd::CMD_STRD_MAT, "STRD_MAT — stored/standard material"),
        (cmd::BLTCMD_HTIME_RD, "HTIME_RD — heat time"),
        (cmd::CMD_RD_HD_YINWEI, "RD_HD_YINWEI — printhead offset"),
        (cmd::CMD_RD_CONLAB_YINWEI, "RD_CONLAB_YINWEI / RD_DEV_DPI"),
        (cmd::CMD_READ_POWER_OFF_TIME, "READ_POWER_OFF_TIME"),
        (cmd::CMD_READ_BUZZER_KEY, "READ_BUZZER_KEY"),
        (cmd::CMD_RD_USER_INF, "RD_USER_INF — user info block"),
        (cmd::CMD_ADJ_READ_DATA, "ADJ_READ_DATA — calibration block"),
        (cmd::CMD_RD_DEV_OPT, "RD_DEV_OPT — device options"),
        (
            cmd::BLTCMD_RD_DEV_PAR,
            "BLTCMD_RD_DEV_PAR — device parameters",
        ),
        (cmd::CMD_RD_TIMESTAMP, "RD_TIMESTAMP"),
        (cmd::CMD_ADJ_RD_CONTINUE, "ADJ_RD_CONTINUE"),
        (cmd::CMD_MAT_AUTHEN_RESULT, "MAT_AUTHEN_RESULT"),
        (cmd::CMD_CHECK_RIB, "CHECK_RIB — ribbon"),
        (cmd::CMD_RD_LAB_DPI, "RD_LAB_DPI"),
    ];

    let control = printer
        .probe_raw(cmd::CMD_CHECK_DEVICE, 0)
        .await?
        .ok_or("control probe got no response — check the link before trusting results")?;
    eprintln!(
        "control   0x12 CHECK_DEVICE  {:02x?}",
        head(&control, COMPARE_LEN)
    );

    // Calibrate the negative before trusting any positive.
    let baseline = calibrate_baseline(printer).await?;
    eprintln!();

    let extra: Vec<(u8, &str)> = also.iter().map(|&op| (op, "(requested)")).collect();
    let mut implemented = Vec::new();

    for &(op, label) in READS.iter().chain(extra.iter()) {
        match printer.probe_raw(op, 0).await? {
            Some(r) => {
                let h = head(&r, COMPARE_LEN);
                let differs = h != baseline.as_slice();
                eprintln!(
                    "0x{op:02X} {label:38} {} {:02x?}",
                    if differs { "IMPL " } else { "  -  " },
                    h
                );
                if differs {
                    implemented.push((op, label, ascii_of(h)));
                }
            }
            None => eprintln!("0x{op:02X} {label:38}   -   (no response)"),
        }

        if let Some(st) = printer.query_status().await?
            && let Some(errs) = st.error_description()
        {
            return Err(format!("aborting after 0x{op:02X}: printer reports {errs}").into());
        }
    }

    eprintln!(
        "\n{} opcode(s) answered differently from unallocated:",
        implemented.len()
    );
    for (op, label, ascii) in &implemented {
        eprintln!("  0x{op:02X} {label}{}", ascii.as_deref().unwrap_or(""));
    }
    Ok(())
}

/// What "not implemented" looks like: the head of the reply to unallocated
/// opcodes, warning if they don't agree.
async fn calibrate_baseline(printer: &Printer) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut baseline: Option<Vec<u8>> = None;
    for op in UNALLOCATED {
        let Some(r) = printer.probe_raw(op, 0).await? else {
            continue;
        };
        let h = head(&r, COMPARE_LEN);
        eprintln!("baseline  0x{op:02X} (unallocated) {h:02x?}");
        match &baseline {
            None => baseline = Some(h.to_vec()),
            Some(b) if b != h => {
                eprintln!("  note: unallocated opcodes disagree; classification is unreliable");
            }
            _ => {}
        }
    }
    Ok(baseline.ok_or("unallocated opcodes gave no response; cannot calibrate")?)
}

pub async fn paper_back(printer: &Printer, param: u16) -> CliResult {
    let baseline = calibrate_baseline(printer).await?;

    let before = printer.query_status().await?;
    eprintln!("Sending PAPER_BACK (0xBA) param={param} — watch the printer.");

    let resp = printer.probe_raw(cmd::CMD_PAPER_BACK, param).await?;
    match resp {
        None => eprintln!("PAPER_BACK -> (no response): not implemented"),
        Some(r) => {
            let h = head(&r, COMPARE_LEN);
            if h == baseline.as_slice() {
                eprintln!("PAPER_BACK -> {h:02x?}");
                eprintln!("Identical to an unallocated opcode: NOT implemented, nothing moved.");
            } else {
                eprintln!("PAPER_BACK -> {h:02x?}");
                eprintln!(
                    "Differs from the unallocated baseline — the firmware knows this opcode."
                );
            }
        }
    }

    let after = printer.query_status().await?;
    match (before, after) {
        (Some(b), Some(a)) => {
            eprintln!(
                "status: print_count {} -> {}, errors {:?} -> {:?}",
                b.print_count,
                a.print_count,
                b.error_description(),
                a.error_description()
            );
        }
        _ => eprintln!("status: unavailable"),
    }
    Ok(())
}

fn head(resp: &[u8], n: usize) -> &[u8] {
    &resp[..resp.len().min(n)]
}

/// Surface any printable run in a reply — `RD_TIMESTAMP` answers in ASCII, and
/// others may too.
fn ascii_of(bytes: &[u8]) -> Option<String> {
    let run: String = bytes
        .iter()
        .filter(|b| b.is_ascii_graphic())
        .map(|&b| b as char)
        .collect();
    (run.len() >= 4).then(|| format!("  ascii={run:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The deny-list is a safety boundary, not a convenience: probe-reads must
    /// refuse anything that writes, moves paper, or could enter the bootloader,
    /// no matter what the caller asks for.
    #[test]
    fn probe_deny_list_refuses_dangerous_opcodes() {
        for op in [
            cmd::CMD_ADJ_RESTORE_FACTORY,
            cmd::CMD_PAPER_BACK,
            cmd::CMD_PAPER_SKIP,
            cmd::CMD_SET_RFID_DATA,
            cmd::CMD_WR_DEV_OPT,
            cmd::BLTCMD_HTIME_SET,
            cmd::CMD_START_PRINT,
        ] {
            assert!(!is_probe_safe(op), "0x{op:02X} should be refused");
            assert!(parse_opcode(&format!("0x{op:02X}")).is_err());
        }

        // The whole firmware range, not just the opcodes we happen to know.
        for op in 0xC0u8..=0xEF {
            assert!(!is_probe_safe(op), "0x{op:02X} is firmware range");
        }
    }

    #[test]
    fn probe_accepts_reads_in_both_radixes() {
        assert!(is_probe_safe(cmd::BLTCMD_HTIME_RD));
        assert_eq!(parse_opcode("0x2B"), Ok(cmd::BLTCMD_HTIME_RD));
        assert_eq!(parse_opcode("43"), Ok(cmd::BLTCMD_HTIME_RD));
        assert!(parse_opcode("zzz").is_err());
    }
}
