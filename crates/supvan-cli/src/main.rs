//! `supvan-cli` — a diagnostic tool for talking to a Supvan printer directly,
//! bypassing the IPP/CUPS stack. Connect over Bluetooth (an address) or USB HID
//! (a `/dev/hidrawN` path) and run a subcommand: `probe` (device/status/material/
//! version), `material` (loaded label + RFID + remaining count), `test-print`
//! (a built-in pattern), `feed` (advance one label), `provision` (inject a
//! synthetic material record for stock the printer can't read a tag from),
//! `heat-sweep` (walk heat time against density to calibrate unknown stock),
//! `two-color` (test the per-dot red/black mode), `gray-ramp` (compare halftone
//! kernels), or `discover` (scan for Supvan Bluetooth devices).

use std::error::Error;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use supvan_proto::bitmap::{
    CardPattern, PRINTHEAD_WIDTH_MM, create_gray_bands, create_gray_ramp, create_two_colour_pattern,
};
use supvan_proto::buffer::{Density, PageOptions};
use supvan_proto::cmd;
use supvan_proto::dither::DitherMode;
use supvan_proto::printer::Printer;
use supvan_proto::profile::PrintProfile;
use supvan_proto::rfid::{RfidMaterial, heat_presets};
use supvan_proto::status::{DEFAULT_LABEL_GAP_MM, DEFAULT_LABEL_HEIGHT_MM, MaterialInfo};

type CliResult = Result<(), Box<dyn Error>>;

#[derive(Parser)]
#[command(name = "supvan-cli", about = "Supvan T50 Pro printer tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Label geometry, shared by every subcommand that writes a material record.
#[derive(clap::Args, Clone, Copy)]
struct LabelArgs {
    /// Label width across the printhead, mm
    #[arg(long, default_value_t = 40)]
    width: u8,
    /// Label length along the feed direction, mm
    #[arg(long, default_value_t = 30)]
    length: u8,
    /// Inter-label gap, mm
    #[arg(long, default_value_t = DEFAULT_LABEL_GAP_MM)]
    gap: u8,
}

#[derive(Subcommand)]
enum Command {
    /// Probe printer: check device, status, material, version info
    Probe {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
    },
    /// Query and print label material info
    Material {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
    },
    /// Send a test print pattern
    TestPrint {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        /// Black print density (0-15)
        #[arg(short, long, default_value_t = 4)]
        density: u8,
        /// Red print density (0-15); defaults to matching --density
        #[arg(long)]
        red_density: Option<u8>,
        /// Set the undocumented PAGE_REG_BITS savepaper bit (省纸). Neither
        /// vendor tool ever sets it; the guess under test is that it suppresses
        /// the advance to the tear-off position.
        #[arg(long)]
        save_paper: bool,
        /// Use the E-series print protocol.
        #[arg(long)]
        e_series: bool,
    },
    /// Feed/advance one blank label (PAPER_SKIP)
    Feed {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
    },
    /// Write a synthetic label-material record, for stock whose RFID tag the
    /// printer can't read (third-party or foreign-brand rolls)
    Provision {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        #[command(flatten)]
        label: LabelArgs,
        /// Heat times as `heat5:heat40`; defaults to the vendor's standard profile
        #[arg(long, value_parser = parse_heat_pair)]
        heat: Option<(u16, u16)>,
        /// Labels remaining to report on the roll
        #[arg(long, default_value_t = 480)]
        count: u32,
        /// Consumable catalogue code. The vendor treats 5602, 5618-5621 and
        /// 5686-5692 as two-colour stock; 30000 is its placeholder for
        /// "not a catalogue item".
        #[arg(long, default_value_t = 30000)]
        code: u16,
        /// Material type discriminant (1 = die-cut, 0 = continuous)
        #[arg(long, default_value_t = 1)]
        mat_type: u8,
    },
    /// Sweep heat time against density, printing one calibration strip per heat
    /// profile. For two-colour thermal stock, this finds the energy at which the
    /// colour flips.
    HeatSweep {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        #[command(flatten)]
        label: LabelArgs,
        /// Heat profiles to walk, each `heat5:heat40`. Repeatable; defaults to
        /// the three the vendor ships.
        #[arg(long = "heat", value_parser = parse_heat_pair)]
        heats: Vec<(u16, u16)>,
        /// Densities to lay down the strip, top to bottom. Each entry is either
        /// `N` (both trims at N) or `BLACK:RED` to drive them independently.
        #[arg(long, value_delimiter = ',', value_parser = parse_density,
              default_value = "0,2,4,6,8,10,12,15")]
        densities: Vec<Density>,
    },
    /// Print a two-colour test card: a thick red bar above a thin black bar.
    /// Tells us whether the firmware honours two-colour mode at all, and which
    /// plane is which.
    TwoColor {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        /// Label width across the printhead, mm
        #[arg(long, default_value_t = 40)]
        width: u8,
        /// Label length along the feed direction, mm
        #[arg(long, default_value_t = 30)]
        length: u8,
        /// Density as `N` or `BLACK:RED`
        #[arg(long, value_parser = parse_density, default_value = "8:4")]
        density: Density,
        /// `bars` = red above black (one colour per line); `stripes` = vertical
        /// red/black alternating, both colours on every line
        #[arg(long, value_parser = parse_pattern, default_value = "bars")]
        pattern: CardPattern,
    },
    /// Print a grayscale staircase through one halftone kernel. The printer is
    /// 1bpp, so every intermediate tone is the dither's doing — this is how you
    /// compare kernels on real stock.
    GrayRamp {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        /// Label width across the printhead, mm
        #[arg(long, default_value_t = 40)]
        width: u8,
        /// Label length along the feed direction, mm
        #[arg(long, default_value_t = 30)]
        length: u8,
        /// Halftone kernel
        #[arg(long, default_value = "bayer")]
        dither: DitherMode,
        /// Grey steps from white to black
        #[arg(long, default_value_t = 8)]
        steps: u32,
        /// Lay the steps as vertical bands across the head instead of down the
        /// feed, so every printhead line carries all of them at once
        #[arg(long)]
        vertical: bool,
        /// Density as `N` or `BLACK:RED`
        #[arg(long, value_parser = parse_density, default_value = "8")]
        density: Density,
    },
    /// Send read-only opcodes and dump the raw responses, to learn which the
    /// firmware implements and what frame shape each returns.
    ///
    /// Reads only. Nothing here writes, moves paper, or touches the firmware
    /// range — see `probe_raw` and the range warnings in `supvan_proto::cmd`.
    ProbeReads {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        /// Extra opcodes to try, hex or decimal (e.g. 0x2B,0xBD). Vetted against
        /// the write/motion/firmware deny-list before sending.
        #[arg(long, value_delimiter = ',', value_parser = parse_opcode)]
        also: Vec<u8>,
    },
    /// Send PAPER_BACK (0xBA) — reverse feed.
    ///
    /// Deliberately its own command rather than an escape hatch in
    /// `probe-reads`, whose whole guarantee is that it cannot move paper. The
    /// app declares this opcode and never sends it, so the parameter meaning is
    /// unknown; the reply is compared against an unallocated opcode first, so a
    /// firmware that doesn't implement it is identified without anything moving.
    PaperBack {
        /// Bluetooth address, `ble://<address>`, or /dev/hidrawN path
        target: String,
        /// Parameter — meaning unknown, plausibly a distance. Starts at 0.
        #[arg(long, default_value_t = 0)]
        param: u16,
    },
    /// Scan for Supvan Bluetooth devices (via BlueZ D-Bus)
    Discover,
}

async fn connect(target: &str) -> Result<Printer, Box<dyn Error>> {
    if target.starts_with("/dev/hidraw") {
        eprintln!("Opening USB HID {target}...");
    } else if target.starts_with("ble:") {
        eprintln!("Connecting to {target} (BLE GATT)...");
    } else {
        eprintln!("Connecting to {target} (Bluetooth)...");
    }
    let printer = Printer::open_target(target).await?;
    eprintln!("Connected.");
    Ok(printer)
}

/// Every status flag, plus the raw registers.
///
/// The firmware sets bits we have no name for, so the registers are shown as
/// well and any undecoded bit is called out — a print that stops with all the
/// known flags clear is precisely when those matter.
fn print_status(s: &supvan_proto::status::PrinterStatus) {
    for (name, value) in [
        ("printing", s.printing),
        ("device_busy", s.device_busy),
        ("buf_full", s.buf_full),
        ("cover_open", s.cover_open),
        ("insert_usb", s.insert_usb),
        ("label_end", s.label_end),
        ("label_not_installed", s.label_not_installed),
        ("label_rw_error", s.label_rw_error),
        ("label_mode_error", s.label_mode_error),
        ("ribbon_end", s.ribbon_end),
        ("ribbon_rw_error", s.ribbon_rw_error),
        ("head_temp_high", s.head_temp_high),
        ("low_battery", s.low_battery),
    ] {
        eprintln!("  {name:<20} {value}");
    }
    eprintln!("  {:<20} {}", "print_count", s.print_count);

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

async fn cmd_probe(target: &str) -> CliResult {
    let printer = connect(target).await?;

    if printer.check_device().await? {
        eprintln!("Device: OK");
    } else {
        return Err("device check: no response".into());
    }

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
        eprintln!("Material:");
        eprintln!("  Label:     {}mm x {}mm", mat.width_mm, mat.height_mm);
        eprintln!("  Type:      {}", mat.label_type);
        eprintln!("  Gap:       {}mm", mat.gap_mm);
        eprintln!("  SN:        {}", mat.sn);
        eprintln!("  UUID:      {}", mat.uuid);
        eprintln!("  Code:      {}", mat.code);
        if let Some(remaining) = mat.remaining {
            eprintln!("  Remaining: {remaining} labels");
        }
        if let Some(ref dev_sn) = mat.device_sn {
            eprintln!("  Device SN: {dev_sn}");
        }
    }
    Ok(())
}

async fn cmd_material(target: &str) -> CliResult {
    let printer = connect(target).await?;

    if !printer.check_device().await? {
        return Err("device not responding".into());
    }

    let mat = printer
        .query_material()
        .await?
        .ok_or("no material info (label not installed?)")?;

    println!(
        "Label:     {}mm x {}mm  (type={}, gap={}mm)",
        mat.width_mm, mat.height_mm, mat.label_type, mat.gap_mm
    );
    println!("Label SN:  {}", mat.sn);
    println!("RFID UID:  {}", mat.uuid);
    println!("RFID code: {}", mat.code);
    match mat.remaining {
        Some(r) => println!("Remaining: {r} labels"),
        None => println!("Remaining: (not reported)"),
    }
    match mat.device_sn {
        Some(s) => println!("Device SN: {s}"),
        None => println!("Device SN: (not in this response)"),
    }
    Ok(())
}

async fn cmd_test_print(
    target: &str,
    density: Density,
    save_paper: bool,
    e_series: bool,
) -> CliResult {
    let mut printer = connect(target).await?;

    if e_series {
        printer.set_profile(PrintProfile::ESeries);
    }

    // Query material to get label dimensions, falling back to printhead-width
    // defaults if no label is installed.
    let mat = match printer.query_material().await? {
        Some(m) => m,
        None => {
            eprintln!(
                "No material info, using defaults ({PRINTHEAD_WIDTH_MM}mm x {DEFAULT_LABEL_HEIGHT_MM}mm)"
            );
            MaterialInfo {
                width_mm: PRINTHEAD_WIDTH_MM as u8,
                height_mm: DEFAULT_LABEL_HEIGHT_MM,
                gap_mm: DEFAULT_LABEL_GAP_MM,
                ..Default::default()
            }
        }
    };

    eprintln!(
        "Printing test pattern on {}mm x {}mm label...",
        mat.width_mm, mat.height_mm
    );
    printer
        .test_print(
            &mat,
            density,
            PageOptions {
                save_paper,
                ..Default::default()
            },
        )
        .await?;
    eprintln!("Done.");
    Ok(())
}

async fn cmd_feed(target: &str) -> CliResult {
    let printer = connect(target).await?;
    printer.paper_skip().await?;
    eprintln!("Fed one label.");
    Ok(())
}

/// Parse a density entry: `N` sets both trims, `BLACK:RED` sets them apart.
fn parse_density(s: &str) -> Result<Density, String> {
    match s.split_once(':') {
        Some((black, red)) => Ok(Density {
            black: black.parse().map_err(|_| format!("bad black `{black}`"))?,
            red: red.parse().map_err(|_| format!("bad red `{red}`"))?,
        }),
        None => Ok(Density::uniform(
            s.parse().map_err(|_| format!("bad density `{s}`"))?,
        )),
    }
}

/// Parse a `heat5:heat40` pair, e.g. `1700:1200`.
fn parse_heat_pair(s: &str) -> Result<(u16, u16), String> {
    let (h5, h40) = s
        .split_once(':')
        .ok_or_else(|| format!("expected `heat5:heat40`, got `{s}`"))?;
    Ok((
        h5.parse().map_err(|_| format!("bad heat5 `{h5}`"))?,
        h40.parse().map_err(|_| format!("bad heat40 `{h40}`"))?,
    ))
}

fn build_material(
    label: LabelArgs,
    heat: (u16, u16),
    count: u32,
    mat_type: u8,
    code: u16,
) -> RfidMaterial {
    RfidMaterial {
        width_mm: label.width,
        length_mm: label.length,
        gap_mm: label.gap,
        heat_time_5: heat.0,
        heat_time_40: heat.1,
        remaining: count,
        mat_type,
        code,
        ..Default::default()
    }
}

/// Write the record and report whether the printer took it. The write-then-
/// settle-poll dance lives in `Printer::provision_material`, shared with the
/// IPP app.
async fn provision(printer: &Printer, mat: &RfidMaterial) -> Result<(), Box<dyn Error>> {
    match printer.provision_material(mat).await {
        Ok(m) => {
            eprintln!(
                "Provisioned: {}mm x {}mm, gap {}mm, heat {}/{}, UUID {}",
                m.width_mm, m.height_mm, m.gap_mm, mat.heat_time_5, mat.heat_time_40, m.uuid
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Warning: {e}");
            Ok(())
        }
    }
}

async fn cmd_provision(
    target: &str,
    label: LabelArgs,
    heat: Option<(u16, u16)>,
    count: u32,
    mat_type: u8,
    code: u16,
) -> CliResult {
    let printer = connect(target).await?;
    let heat = heat.unwrap_or(heat_presets::STANDARD);
    let mat = build_material(label, heat, count, mat_type, code);
    provision(&printer, &mat).await
}

#[allow(clippy::too_many_arguments)]
async fn cmd_gray_ramp(
    target: &str,
    width: u8,
    length: u8,
    dither: DitherMode,
    steps: u32,
    density: Density,
    vertical: bool,
) -> CliResult {
    let printer = connect(target).await?;
    let (gray, w, h) = if vertical {
        create_gray_bands(width as u32, length as u32, steps)
    } else {
        create_gray_ramp(width as u32, length as u32, steps)
    };
    let layout = if vertical {
        "vertical bands across the head"
    } else {
        "steps down the feed"
    };
    eprintln!(
        "Grey ramp on {width}mm x {length}mm: {steps} {layout}, white->black, \
dither={dither:?}, density={density}."
    );
    printer
        .print_grayscale(&gray, w, h, density, dither)
        .await?;
    eprintln!("Done.");
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

fn parse_opcode(s: &str) -> Result<u8, String> {
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

async fn cmd_probe_reads(target: &str, also: Vec<u8>) -> CliResult {
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

    /// Opcodes from the middle of the largest unallocated hole. Used only to
    /// learn what "not implemented" looks like on the wire — USB HID does not
    /// echo the command byte (`usb_transport::validate_response`), so a
    /// non-empty response proves nothing on its own and the reply *content* is
    /// the only discriminator.
    const UNALLOCATED: [u8; 2] = [0x50, 0x51];

    /// Trailing bytes of every reply are device-constant; only the head varies.
    const COMPARE_LEN: usize = 16;

    let printer = connect(target).await?;

    let control = printer
        .probe_raw(cmd::CMD_CHECK_DEVICE, 0)
        .await?
        .ok_or("control probe got no response — check the link before trusting results")?;
    eprintln!(
        "control   0x12 CHECK_DEVICE  {:02x?}",
        head(&control, COMPARE_LEN)
    );

    // Calibrate the negative before trusting any positive.
    let mut baseline = None;
    for op in UNALLOCATED {
        if let Some(r) = printer.probe_raw(op, 0).await? {
            eprintln!(
                "baseline  0x{op:02X} (unallocated) {:02x?}",
                head(&r, COMPARE_LEN)
            );
            match &baseline {
                None => baseline = Some(head(&r, COMPARE_LEN).to_vec()),
                Some(b) if b != head(&r, COMPARE_LEN) => {
                    eprintln!("  note: unallocated opcodes disagree; classification is unreliable");
                }
                _ => {}
            }
        }
    }
    let Some(baseline) = baseline else {
        return Err("unallocated opcodes gave no response; cannot calibrate".into());
    };
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

async fn cmd_paper_back(target: &str, param: u16) -> CliResult {
    const COMPARE_LEN: usize = 16;
    /// Middle of the largest unallocated hole — what "not implemented" looks like.
    const UNALLOCATED: u8 = 0x50;

    let printer = connect(target).await?;

    let baseline = printer
        .probe_raw(UNALLOCATED, 0)
        .await?
        .ok_or("unallocated opcode gave no response; cannot calibrate")?;
    let baseline = head(&baseline, COMPARE_LEN).to_vec();
    eprintln!("baseline (unallocated 0x{UNALLOCATED:02X}): {baseline:02x?}");

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

/// Parse the test-card pattern name.
fn parse_pattern(s: &str) -> Result<CardPattern, String> {
    match s {
        "bars" => Ok(CardPattern::Bars),
        "stripes" => Ok(CardPattern::Stripes),
        other => Err(format!("unknown pattern `{other}` (bars|stripes)")),
    }
}

async fn cmd_two_color(
    target: &str,
    width: u8,
    length: u8,
    density: Density,
    pattern: CardPattern,
) -> CliResult {
    let printer = connect(target).await?;
    let (rgb, w, h) = create_two_colour_pattern(width as u32, length as u32, pattern);

    match pattern {
        CardPattern::Bars => eprintln!(
            "Bars on {width}mm x {length}mm: thick RED above thin BLACK, black={} red={}.",
            density.black, density.red
        ),
        CardPattern::Stripes => {
            eprintln!(
                "Stripes on {width}mm x {length}mm: 4 vertical bars, RED BLACK RED BLACK, \
black={} red={}.",
                density.black, density.red
            );
            eprintln!(
                "Both colours share every printhead line — only real two-plane support can do this."
            );
        }
    }
    printer.print_two_colour(&rgb, w, h, density).await?;
    eprintln!("Done.");
    Ok(())
}

async fn cmd_heat_sweep(
    target: &str,
    label: LabelArgs,
    heats: Vec<(u16, u16)>,
    densities: Vec<Density>,
) -> CliResult {
    let heats = if heats.is_empty() {
        vec![
            heat_presets::STANDARD,
            heat_presets::BLACK_MARK,
            heat_presets::CARDSTOCK,
        ]
    } else {
        heats
    };

    let printer = connect(target).await?;
    eprintln!(
        "Sweeping {} heat profiles x {} densities on {}mm x {}mm labels.",
        label.width,
        label.length,
        heats.len(),
        densities.len()
    );
    eprintln!("Bands run top to bottom in the order printed; annotate each strip as it comes out.");

    for (i, heat) in heats.iter().enumerate() {
        let mat = build_material(label, *heat, 480, 1, 30000);
        eprintln!(
            "\nStrip {}/{}: heat5={} heat40={}, densities {}",
            i + 1,
            heats.len(),
            heat.0,
            heat.1,
            densities
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        provision(&printer, &mat).await?;
        printer
            .print_swatch_ladder(label.width as u32, label.length as u32, &densities)
            .await?;
    }

    eprintln!("\nSweep complete: {} strips.", heats.len());
    Ok(())
}

/// One line per device, per transport, with the target string to paste into
/// any other subcommand.
async fn cmd_discover() -> CliResult {
    supvan_discover::models::load();

    eprintln!("Scanning USB, Bluetooth and BLE...");
    let usb = supvan_discover::usb::list_candidates().await;
    let bt = supvan_discover::bt::list_candidates();
    let ble = supvan_discover::ble::list_candidates().await;
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

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::Probe { target } => cmd_probe(&target).await,
        Command::Material { target } => cmd_material(&target).await,
        Command::TestPrint {
            target,
            density,
            red_density,
            save_paper,
            e_series,
        } => {
            cmd_test_print(
                &target,
                Density {
                    black: density,
                    red: red_density.unwrap_or(density),
                },
                save_paper,
                e_series,
            )
            .await
        }
        Command::Feed { target } => cmd_feed(&target).await,
        Command::Provision {
            target,
            label,
            heat,
            count,
            code,
            mat_type,
        } => cmd_provision(&target, label, heat, count, mat_type, code).await,
        Command::HeatSweep {
            target,
            label,
            heats,
            densities,
        } => cmd_heat_sweep(&target, label, heats, densities).await,
        Command::TwoColor {
            target,
            width,
            length,
            density,
            pattern,
        } => cmd_two_color(&target, width, length, density, pattern).await,
        Command::GrayRamp {
            target,
            width,
            length,
            dither,
            steps,
            density,
            vertical,
        } => cmd_gray_ramp(&target, width, length, dither, steps, density, vertical).await,
        Command::ProbeReads { target, also } => cmd_probe_reads(&target, also).await,
        Command::PaperBack { target, param } => cmd_paper_back(&target, param).await,
        Command::Discover => cmd_discover().await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;
    use supvan_proto::buffer::Density;
    use supvan_proto::cmd;

    #[test]
    fn parse_probe_with_target() {
        let cli = Cli::try_parse_from(["supvan-cli", "probe", "/dev/hidraw3"]).unwrap();
        match cli.command {
            Command::Probe { target } => assert_eq!(target, "/dev/hidraw3"),
            _ => panic!("expected Probe"),
        }
    }

    #[test]
    fn probe_requires_target() {
        // `target` is a required positional now (no hardcoded default).
        assert!(Cli::try_parse_from(["supvan-cli", "probe"]).is_err());
    }

    #[test]
    fn parse_test_print_density() {
        let cli = Cli::try_parse_from([
            "supvan-cli",
            "test-print",
            "AA:BB:CC:DD:EE:FF",
            "--density",
            "7",
        ])
        .unwrap();
        match cli.command {
            Command::TestPrint {
                target, density, ..
            } => {
                assert_eq!(target, "AA:BB:CC:DD:EE:FF");
                assert_eq!(density, 7);
            }
            _ => panic!("expected TestPrint"),
        }
    }

    #[test]
    fn parse_feed_with_target() {
        let cli = Cli::try_parse_from(["supvan-cli", "feed", "/dev/hidraw3"]).unwrap();
        match cli.command {
            Command::Feed { target } => assert_eq!(target, "/dev/hidraw3"),
            _ => panic!("expected Feed"),
        }
    }

    #[test]
    fn parse_discover() {
        let cli = Cli::try_parse_from(["supvan-cli", "discover"]).unwrap();
        assert!(matches!(cli.command, Command::Discover));
    }

    #[test]
    fn parse_provision_with_heat() {
        let cli = Cli::try_parse_from([
            "supvan-cli",
            "provision",
            "/dev/hidraw11",
            "--width",
            "50",
            "--length",
            "30",
            "--heat",
            "1900:1400",
        ])
        .unwrap();
        let Command::Provision { label, heat, .. } = cli.command else {
            panic!("expected Provision");
        };
        assert_eq!((label.width, label.length), (50, 30));
        assert_eq!(heat, Some((1900, 1400)));
    }

    /// The catalogue code is how we claim two-colour stock (5618) instead of
    /// the 30000 placeholder, so its default and override both matter.
    #[test]
    fn provision_code_defaults_and_overrides() {
        let cli = Cli::try_parse_from(["supvan-cli", "provision", "/dev/hidraw11"]).unwrap();
        let Command::Provision { code, .. } = cli.command else {
            panic!("expected Provision");
        };
        assert_eq!(code, 30000);

        let cli =
            Cli::try_parse_from(["supvan-cli", "provision", "/dev/hidraw11", "--code", "5618"])
                .unwrap();
        let Command::Provision { code, .. } = cli.command else {
            panic!("expected Provision");
        };
        assert_eq!(code, 5618);
    }

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
            assert!(!super::is_probe_safe(op), "0x{op:02X} should be refused");
            assert!(super::parse_opcode(&format!("0x{op:02X}")).is_err());
        }

        // The whole firmware range, not just the opcodes we happen to know.
        for op in 0xC0u8..=0xEF {
            assert!(!super::is_probe_safe(op), "0x{op:02X} is firmware range");
        }
    }

    #[test]
    fn probe_accepts_reads_in_both_radixes() {
        assert!(super::is_probe_safe(cmd::BLTCMD_HTIME_RD));
        assert_eq!(super::parse_opcode("0x2B"), Ok(cmd::BLTCMD_HTIME_RD));
        assert_eq!(super::parse_opcode("43"), Ok(cmd::BLTCMD_HTIME_RD));
        assert!(super::parse_opcode("zzz").is_err());
    }

    #[test]
    fn parse_gray_ramp_dither_mode() {
        let cli = Cli::try_parse_from([
            "supvan-cli",
            "gray-ramp",
            "/dev/hidraw11",
            "--dither",
            "atkinson",
            "--steps",
            "12",
        ])
        .unwrap();
        let Command::GrayRamp { dither, steps, .. } = cli.command else {
            panic!("expected GrayRamp");
        };
        assert_eq!(dither, supvan_proto::dither::DitherMode::Atkinson);
        assert_eq!(steps, 12);

        assert!(Cli::try_parse_from(["supvan-cli", "gray-ramp", "x", "--dither", "nope"]).is_err());
    }

    #[test]
    fn parse_two_color_density_pair() {
        let cli = Cli::try_parse_from([
            "supvan-cli",
            "two-color",
            "/dev/hidraw11",
            "--width",
            "34",
            "--length",
            "34",
            "--density",
            "10:3",
        ])
        .unwrap();
        let Command::TwoColor {
            width,
            length,
            density,
            ..
        } = cli.command
        else {
            panic!("expected TwoColor");
        };
        assert_eq!((width, length), (34, 34));
        assert_eq!(density, Density { black: 10, red: 3 });
    }

    /// `N` sets both trims; `BLACK:RED` drives them apart. The split form is the
    /// whole point of the type — black and red live in different header fields.
    #[test]
    fn density_accepts_both_forms() {
        assert_eq!(super::parse_density("6"), Ok(Density::uniform(6)));
        assert_eq!(
            super::parse_density("3:12"),
            Ok(Density { black: 3, red: 12 })
        );
        assert!(super::parse_density("3:").is_err());
        assert!(super::parse_density("x").is_err());
    }

    #[test]
    fn heat_pair_needs_a_colon() {
        assert!(super::parse_heat_pair("1700").is_err());
        assert!(super::parse_heat_pair("1700:abc").is_err());
        assert_eq!(super::parse_heat_pair("1700:1200"), Ok((1700, 1200)));
    }

    #[test]
    fn parse_heat_sweep_repeats_heat_and_splits_densities() {
        let cli = Cli::try_parse_from([
            "supvan-cli",
            "heat-sweep",
            "/dev/hidraw11",
            "--heat",
            "1700:1200",
            "--heat",
            "2500:2000",
            "--densities",
            "0,4,8,15",
        ])
        .unwrap();
        let Command::HeatSweep {
            heats, densities, ..
        } = cli.command
        else {
            panic!("expected HeatSweep");
        };
        assert_eq!(heats, vec![(1700, 1200), (2500, 2000)]);
        assert_eq!(densities, [0, 4, 8, 15].map(Density::uniform).to_vec());
    }

    /// Omitting --heat is what selects the three vendor presets at run time, so
    /// the parsed value must stay empty rather than picking up a clap default.
    #[test]
    fn heat_sweep_defaults_to_no_explicit_heats() {
        let cli = Cli::try_parse_from(["supvan-cli", "heat-sweep", "/dev/hidraw11"]).unwrap();
        let Command::HeatSweep {
            heats, densities, ..
        } = cli.command
        else {
            panic!("expected HeatSweep");
        };
        assert!(heats.is_empty());
        assert_eq!(
            densities,
            [0, 2, 4, 6, 8, 10, 12, 15].map(Density::uniform).to_vec()
        );
    }
}
