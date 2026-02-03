# Supvan T50 Pro Printer Driver

Linux printer driver for the Supvan T50 Pro thermal label printer
(also compatible with Katasymbol M50 Pro). Provides an IPP Everywhere
printer application via [PAPPL](https://www.msweet.org/pappl/) and a
command-line diagnostic tool.

The printer protocol was reverse-engineered from the Katasymbol Android
app (v1.4.20).

## Compatible Devices

- Supvan T50 Plus / T50 series
- Katasymbol M50 Pro
- Other devices advertising Bluetooth names containing T50, T0117,
  Supvan, or Katasymbol

## Crate Structure

| Crate | Description |
|-------|-------------|
| `supvan-proto` | Printer protocol: RFCOMM transport, commands, status parsing, bitmap transforms, LZMA compression |
| `pappl-sys` | Bindgen FFI bindings for libpappl and libcups |
| `supvan-app` | PAPPL printer application binary (`supvan-printer-app`) |
| `supvan-cli` | Command-line diagnostic tool (`supvan-cli`) |

## Prerequisites

Rust toolchain (edition 2021) and the following system packages:

```sh
# Debian / Ubuntu
sudo apt install libpappl-dev libcups2-dev pkg-config libclang-dev libdbus-1-dev bluez
```

- **libpappl-dev**, **libcups2-dev** -- printer application framework and CUPS libraries
- **libclang-dev** -- required by bindgen to generate FFI bindings
- **libdbus-1-dev** -- BlueZ D-Bus discovery
- **bluez** -- Bluetooth stack (runtime)

## Building

```sh
cargo build --release
```

Binaries:

- `target/release/supvan-printer-app`
- `target/release/supvan-cli`

## Usage

### Printer Application

The printer application runs an IPP server with a web interface on port 8631.
It auto-discovers Bluetooth printers via BlueZ and registers them as IPP
printers that any Linux application can print to.

```sh
# Start the server
./target/release/supvan-printer-app server

# Web interface at http://localhost:8631/
```

Other PAPPL subcommands are available (`devices`, `printers`, `status`,
`submit`, `shutdown`, etc.). Run without arguments for help.

#### Systemd User Service

Create `~/.config/systemd/user/supvan-printer-app.service`:

```ini
[Unit]
Description=Supvan T50 Pro Printer Application
After=bluetooth.target dbus.socket

[Service]
Type=simple
ExecStart=/path/to/supvan-printer-app server
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Then:

```sh
systemctl --user daemon-reload
systemctl --user enable --now supvan-printer-app
```

### CLI Tool

Direct printer interaction over Bluetooth for diagnostics and testing.

```sh
# Discover nearby printers
supvan-cli discover

# Probe a printer (status, material, firmware)
supvan-cli probe AA:BB:CC:DD:EE:FF

# Query loaded label info
supvan-cli material AA:BB:CC:DD:EE:FF

# Send a test print
supvan-cli test-print AA:BB:CC:DD:EE:FF --density 4
```

## Debug Dumps

Set `SUPVAN_DUMP_DIR` to capture raster images at each pipeline stage:

```sh
export SUPVAN_DUMP_DIR=~/.local/state/supvan-dumps
mkdir -p "$SUPVAN_DUMP_DIR"
```

Each print job produces:

| File | Format | Contents |
|------|--------|----------|
| `supvan_NNNN_pre.pgm` | PGM P5 (8bpp) | Pre-dither grayscale input from PAPPL |
| `supvan_NNNN.pbm` | PBM P4 (1bpp) | Post-dither label-sized bitmap |
| `supvan_NNNN_printhead.pbm` | PBM P4 (1bpp) | Final 384-dot-wide printhead image |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level (`debug`, `info`, `warn`, `error`) |
| `SUPVAN_DUMP_DIR` | Directory for debug image dumps |
| `SUPVAN_MOCK` | Set to `1` to run without a real printer |
| `XDG_STATE_HOME` | Override state file location (default: `~/.local/state`) |

## License

MIT
