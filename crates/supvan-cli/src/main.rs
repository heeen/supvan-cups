use clap::{Parser, Subcommand};
use std::process;
use supvan_proto::printer::Printer;
use supvan_proto::rfcomm::RfcommSocket;

const DEFAULT_BT_ADDR: &str = "A4:93:40:A0:87:57";

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
        /// Bluetooth address (XX:XX:XX:XX:XX:XX)
        #[arg(default_value = DEFAULT_BT_ADDR)]
        addr: String,
    },
    /// Query and print label material info
    Material {
        /// Bluetooth address (XX:XX:XX:XX:XX:XX)
        #[arg(default_value = DEFAULT_BT_ADDR)]
        addr: String,
    },
    /// Send a test print pattern
    TestPrint {
        /// Bluetooth address (XX:XX:XX:XX:XX:XX)
        #[arg(default_value = DEFAULT_BT_ADDR)]
        addr: String,
        /// Print density (0-15)
        #[arg(short, long, default_value_t = 4)]
        density: u8,
    },
    /// Scan for Supvan Bluetooth devices (via BlueZ D-Bus)
    Discover,
}

fn connect(addr: &str) -> Printer {
    eprintln!("Connecting to {addr}...");
    let sock = RfcommSocket::connect_default(addr).unwrap_or_else(|e| {
        eprintln!("Connection failed: {e}");
        eprintln!("Is the printer on and paired? bluetoothctl info {addr}");
        process::exit(1);
    });
    eprintln!("Connected.");
    Printer::new(sock)
}

fn cmd_probe(addr: &str) {
    let printer = connect(addr);

    // Check device
    match printer.check_device() {
        Ok(true) => eprintln!("Device: OK"),
        Ok(false) => {
            eprintln!("Device: no response");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("Device check failed: {e}");
            process::exit(1);
        }
    }

    // Status
    if let Ok(Some(status)) = printer.query_status() {
        eprintln!("Status:");
        eprintln!("  printing:     {}", status.printing);
        eprintln!("  device_busy:  {}", status.device_busy);
        eprintln!("  buf_full:     {}", status.buf_full);
        eprintln!("  low_battery:  {}", status.low_battery);
        eprintln!("  cover_open:   {}", status.cover_open);
        eprintln!("  print_count:  {}", status.print_count);
        if let Some(errs) = status.error_description() {
            eprintln!("  ERRORS:       {errs}");
        }
    }

    // Device name
    if let Ok(Some(name)) = printer.read_device_name() {
        eprintln!("Device name: {name}");
    }

    // Firmware
    if let Ok(Some(fw)) = printer.read_firmware_version() {
        eprintln!("Firmware:    {fw}");
    }

    // Version
    if let Ok(Some(ver)) = printer.read_version() {
        eprintln!("Protocol:    {ver}");
    }

    // Material
    if let Ok(Some(mat)) = printer.query_material() {
        eprintln!("Material:");
        eprintln!("  Label:     {}mm x {}mm", mat.width_mm, mat.height_mm);
        eprintln!("  Type:      {}", mat.label_type);
        eprintln!("  Gap:       {}mm", mat.gap_mm);
        eprintln!("  SN:        {}", mat.sn);
        eprintln!("  UUID:      {}", mat.uuid);
        eprintln!("  Code:      {}", mat.code);
        if let Some(remaining) = mat.remaining {
            eprintln!("  Remaining: {} labels", remaining);
        }
        if let Some(ref dev_sn) = mat.device_sn {
            eprintln!("  Device SN: {dev_sn}");
        }
    }
}

fn cmd_material(addr: &str) {
    let printer = connect(addr);

    if !printer.check_device().unwrap_or(false) {
        eprintln!("Device not responding");
        process::exit(1);
    }

    match printer.query_material() {
        Ok(Some(mat)) => {
            println!(
                "Label: {}mm x {}mm (type={}, gap={}mm)",
                mat.width_mm, mat.height_mm, mat.label_type, mat.gap_mm
            );
            if let Some(remaining) = mat.remaining {
                println!("Remaining: {} labels", remaining);
            }
        }
        Ok(None) => {
            eprintln!("No material info (label not installed?)");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to query material: {e}");
            process::exit(1);
        }
    }
}

fn cmd_test_print(addr: &str, density: u8) {
    let printer = connect(addr);

    // Query material to get label dimensions
    let mat = match printer.query_material() {
        Ok(Some(m)) => m,
        Ok(None) => {
            eprintln!("No material info, using defaults (48mm x 25mm)");
            supvan_proto::status::MaterialInfo {
                uuid: String::new(),
                code: String::new(),
                sn: 0,
                label_type: 0,
                width_mm: 48,
                height_mm: 25,
                gap_mm: 3,
                remaining: None,
                device_sn: None,
            }
        }
        Err(e) => {
            eprintln!("Material query failed: {e}");
            process::exit(1);
        }
    };

    eprintln!(
        "Printing test pattern on {}mm x {}mm label...",
        mat.width_mm, mat.height_mm
    );
    if let Err(e) = printer.test_print(&mat, density) {
        eprintln!("Print failed: {e}");
        process::exit(1);
    }
    eprintln!("Done.");
}

fn cmd_discover() {
    eprintln!("Scanning for Supvan devices...");
    eprintln!("(For full D-Bus discovery, use the CUPS backend with 0 args)");
    eprintln!();
    eprintln!("Manual discovery:");
    eprintln!("  bluetoothctl devices | grep -i 'T0117\\|T50\\|Supvan\\|Katasymbol'");
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match cli.command {
        Command::Probe { addr } => cmd_probe(&addr),
        Command::Material { addr } => cmd_material(&addr),
        Command::TestPrint { addr, density } => cmd_test_print(&addr, density),
        Command::Discover => cmd_discover(),
    }
}
