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

Supported transports:

- **Bluetooth** — RFCOMM (`btrfcomm://` scheme), auto-discovered via BlueZ D-Bus
- **USB HID** — hidraw (`usbhid://` scheme), VID `0x1820` / PID `0x2073`

## Crate Structure

| Crate | Description |
|-------|-------------|
| `supvan-proto` | Printer protocol: Bluetooth RFCOMM and USB HID transports, commands, status parsing, bitmap transforms, LZMA compression |
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

## USB HID Permissions

To use the printer over USB without root, install the udev rule:

```sh
sudo cp etc/udev/rules.d/70-supvan-t50.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

This grants access to logged-in users via the `uaccess` tag. Re-plug the
printer after installing the rule.

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
It auto-discovers printers via BlueZ D-Bus (Bluetooth) and sysfs (USB HID),
and registers them as IPP Everywhere printers that any Linux application can
print to. When both transports are available, USB is preferred.

```sh
# Start the server
./target/release/supvan-printer-app server

# Web interface at http://localhost:8631/

# List discovered devices
./target/release/supvan-printer-app devices
```

Other PAPPL subcommands are available (`printers`, `status`, `submit`,
`shutdown`, etc.). Run without arguments for help.

For automatic CUPS queue creation, enable `cups-browsed`:

```sh
sudo systemctl enable --now cups-browsed
```

#### Systemd User Service

Install the included unit file or create
`~/.config/systemd/user/supvan-printer-app.service`:

```ini
[Unit]
Description=Supvan T50 Pro Printer Application
After=bluetooth.target dbus.socket

[Service]
Type=simple
ExecStart=/path/to/supvan-printer-app server
ExecStopPost=/path/to/cups-cleanup.sh
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

The `ExecStopPost` cleanup script removes stale CUPS queues that were
auto-created by cups-browsed, preventing duplicates across restarts.

Then:

```sh
systemctl --user daemon-reload
systemctl --user enable --now supvan-printer-app
```

### CLI Tool

Direct printer interaction over Bluetooth or USB HID for diagnostics and
testing. Pass a Bluetooth address or `/dev/hidrawN` path as the target.

```sh
# Discover nearby printers
supvan-cli discover

# Probe over Bluetooth
supvan-cli probe AA:BB:CC:DD:EE:FF

# Probe over USB HID
supvan-cli probe /dev/hidraw7

# Query loaded label info
supvan-cli material /dev/hidraw7

# Send a test print
supvan-cli test-print /dev/hidraw7 --density 4
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
