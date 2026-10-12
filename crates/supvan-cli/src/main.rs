//! `supvan-cli` — a diagnostic tool for talking to a Supvan printer directly,
//! bypassing the IPP/CUPS stack. Connect over Bluetooth (an address) or USB HID
//! (a `/dev/hidrawN` path) and run a subcommand: `probe` (device/status/material/
//! version), `material` (loaded label + RFID + remaining count), `test-print`
//! (a built-in pattern), `feed` (advance one label), `provision` (inject a
//! synthetic material record for stock the printer can't read a tag from),
//! `heat-sweep` (walk heat time against density to calibrate unknown stock),
//! `two-color` (test the per-dot red/black mode), `gray-ramp` (compare halftone
//! kernels), or `discover` (scan for Supvan Bluetooth devices).

mod args;
mod discover;
mod print;
mod probe;

use std::error::Error;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use supvan_proto::bitmap::CardPattern;
use supvan_proto::buffer::Density;
use supvan_proto::dither::DitherMode;
use supvan_proto::printer::Printer;
use supvan_proto::rfid::{DEFAULT_ROLL_COUNT, NON_CATALOGUE_CODE};

use args::{LabelArgs, LabelSize, Target, parse_density, parse_heat_pair, parse_pattern};
use probe::parse_opcode;

type CliResult = Result<(), Box<dyn Error>>;

#[derive(Parser)]
#[command(name = "supvan-cli", about = "Supvan T50 Pro printer tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Probe printer: check device, status, material, version info
    Probe {
        #[command(flatten)]
        on: Target,
    },
    /// Query and print label material info
    Material {
        #[command(flatten)]
        on: Target,
    },
    /// Send a test print pattern
    TestPrint {
        #[command(flatten)]
        on: Target,
        /// Density, 0-15, as `N` or `BLACK:RED`
        #[arg(short, long, value_parser = parse_density, default_value = "4")]
        density: Density,
        /// Set the undocumented PAGE_REG_BITS savepaper bit (省纸). Neither
        /// vendor tool ever sets it; the guess under test is that it suppresses
        /// the advance to the tear-off position.
        #[arg(long)]
        save_paper: bool,
    },
    /// Feed/advance one blank label (PAPER_SKIP)
    Feed {
        #[command(flatten)]
        on: Target,
    },
    /// Write a synthetic label-material record, for stock whose RFID tag the
    /// printer can't read (third-party or foreign-brand rolls)
    Provision {
        #[command(flatten)]
        on: Target,
        #[command(flatten)]
        label: LabelArgs,
        /// Heat times as `heat5:heat40`; defaults to the vendor's standard profile
        #[arg(long, value_parser = parse_heat_pair)]
        heat: Option<(u16, u16)>,
        /// Labels remaining to report on the roll
        #[arg(long, default_value_t = DEFAULT_ROLL_COUNT)]
        count: u32,
        /// Consumable catalogue code. The vendor treats 5602, 5618-5621 and
        /// 5686-5692 as two-colour stock; 30000 is its placeholder for
        /// "not a catalogue item".
        #[arg(long, default_value_t = NON_CATALOGUE_CODE)]
        code: u16,
        /// Material type discriminant (1 = die-cut, 0 = continuous)
        #[arg(long, default_value_t = 1)]
        mat_type: u8,
    },
    /// Sweep heat time against density, printing one calibration strip per heat
    /// profile. For two-colour thermal stock, this finds the energy at which the
    /// colour flips.
    HeatSweep {
        #[command(flatten)]
        on: Target,
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
        #[command(flatten)]
        on: Target,
        #[command(flatten)]
        label: LabelSize,
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
        #[command(flatten)]
        on: Target,
        #[command(flatten)]
        label: LabelSize,
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
        #[command(flatten)]
        on: Target,
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
        #[command(flatten)]
        on: Target,
        /// Parameter — meaning unknown, plausibly a distance. Starts at 0.
        #[arg(long, default_value_t = 0)]
        param: u16,
    },
    /// Scan for Supvan printers over USB, Bluetooth and BLE
    Discover,
}

impl Command {
    /// The printer this command talks to; `None` for `discover`.
    fn target(&self) -> Option<&str> {
        match self {
            Self::Probe { on }
            | Self::Material { on }
            | Self::TestPrint { on, .. }
            | Self::Feed { on }
            | Self::Provision { on, .. }
            | Self::HeatSweep { on, .. }
            | Self::TwoColor { on, .. }
            | Self::GrayRamp { on, .. }
            | Self::ProbeReads { on, .. }
            | Self::PaperBack { on, .. } => Some(&on.target),
            Self::Discover => None,
        }
    }
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

async fn run(command: Command) -> CliResult {
    let Some(target) = command.target() else {
        return discover::discover().await;
    };
    let printer = connect(target).await?;
    let p = &printer;
    match command {
        Command::Probe { .. } => probe::probe(p).await,
        Command::Material { .. } => probe::material(p).await,
        Command::TestPrint {
            density,
            save_paper,
            ..
        } => print::test_print(p, density, save_paper).await,
        Command::Feed { .. } => print::feed(p).await,
        Command::Provision {
            label,
            heat,
            count,
            code,
            mat_type,
            ..
        } => print::provision(p, label, heat, count, mat_type, code).await,
        Command::HeatSweep {
            label,
            heats,
            densities,
            ..
        } => print::heat_sweep(p, label, heats, densities).await,
        Command::TwoColor {
            label,
            density,
            pattern,
            ..
        } => print::two_color(p, label, density, pattern).await,
        Command::GrayRamp {
            label,
            dither,
            steps,
            density,
            vertical,
            ..
        } => print::gray_ramp(p, label, dither, steps, density, vertical).await,
        Command::ProbeReads { also, .. } => probe::probe_reads(p, also).await,
        Command::PaperBack { param, .. } => probe::paper_back(p, param).await,
        Command::Discover => unreachable!("handled above: discover takes no target"),
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run(Cli::parse().command).await {
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

    #[test]
    fn parse_probe_with_target() {
        let cli = Cli::try_parse_from(["supvan-cli", "probe", "/dev/hidraw3"]).unwrap();
        match cli.command {
            Command::Probe { on } => assert_eq!(on.target, "/dev/hidraw3"),
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
            Command::TestPrint { on, density, .. } => {
                assert_eq!(on.target, "AA:BB:CC:DD:EE:FF");
                assert_eq!(density, Density::uniform(7));
            }
            _ => panic!("expected TestPrint"),
        }
    }

    #[test]
    fn parse_feed_with_target() {
        let cli = Cli::try_parse_from(["supvan-cli", "feed", "/dev/hidraw3"]).unwrap();
        match cli.command {
            Command::Feed { on } => assert_eq!(on.target, "/dev/hidraw3"),
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
        assert_eq!((label.size.width, label.size.length), (50, 30));
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
        let Command::TwoColor { label, density, .. } = cli.command else {
            panic!("expected TwoColor");
        };
        assert_eq!((label.width, label.length), (34, 34));
        assert_eq!(density, Density { black: 10, red: 3 });
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
