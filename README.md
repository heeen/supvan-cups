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

## Building and Installing

```sh
make build                # cargo build --release
sudo make install         # installs binary, systemd unit, data files, udev rule
sudo udevadm control --reload-rules
```

`make install` places:

| File | Destination |
|------|-------------|
| `supvan-printer-app` | `/usr/bin/` |
| `data/models.toml` | `/usr/share/supvan-printer-app/` |
| `supvan-printer-app.service` | `/usr/lib/systemd/user/` |
| `cups-cleanup.sh`, `cups-register.sh` | `/usr/lib/supvan-printer-app/` |
| `70-supvan-t50.rules` | `/usr/lib/udev/rules.d/` |

Re-plug the USB printer after installing the udev rule.

To uninstall: `sudo make uninstall`.

## Usage

### Printer Application

The printer application runs an IPP server with a web interface on port 8631.
It auto-discovers printers via BlueZ D-Bus (Bluetooth) and sysfs (USB HID),
and registers them as IPP Everywhere printers that any Linux application can
print to. When both transports are available, USB is preferred.

```sh
supvan-printer-app server         # start the server
supvan-printer-app devices        # list discovered devices
# Web interface at http://localhost:8631/
```

Other PAPPL subcommands are available (`printers`, `status`, `submit`,
`shutdown`, etc.). Run without arguments for help.

For automatic CUPS queue creation, enable `cups-browsed`:

```sh
sudo systemctl enable --now cups-browsed
```

#### Systemd User Service

```sh
systemctl --user daemon-reload
systemctl --user enable --now supvan-printer-app
```

The service unit is installed by `make install`. The `ExecStopPost` cleanup
script removes stale CUPS queues that were auto-created by cups-browsed,
preventing duplicates across restarts.

### CLI Tool

Direct printer interaction over Bluetooth or USB HID for diagnostics and
testing. Pass a Bluetooth address or `/dev/hidrawN` path as the target.

```sh
supvan-cli discover                          # discover nearby printers
supvan-cli probe AA:BB:CC:DD:EE:FF           # probe over Bluetooth
supvan-cli probe /dev/hidraw7                # probe over USB HID
supvan-cli material /dev/hidraw7             # query loaded label info
supvan-cli test-print /dev/hidraw7 --density 4
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level (`debug`, `info`, `warn`, `error`) |
| `SUPVAN_MODELS` | Override path to `models.toml` |
| `SUPVAN_DUMP_DIR` | Directory for debug image dumps |
| `SUPVAN_MOCK` | Set to `1` to run without a real printer |
| `XDG_STATE_HOME` | Override state file location (default: `~/.local/state`) |

## License

MIT
