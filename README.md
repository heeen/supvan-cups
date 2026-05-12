# Supvan Printer Driver

Linux printer driver for Supvan thermal label printers. Provides an
IPP Everywhere printer application via [PAPPL](https://www.msweet.org/pappl/)
and a command-line diagnostic tool.

The printer protocol was reverse-engineered from the Katasymbol Android
app (v1.4.20).

## Supported Models

All models use USB VID `0x1820`. Bluetooth and USB HID are auto-discovered.

| Family | Models | DPI | Printhead |
|--------|--------|-----|-----------|
| T50 Series | T50M, T50M Pro, T50M Plus, T50s, T50s Pro | 203 | 48mm / 384 dots |
| T80 Series | T80M, T80M Pro | 201 | 72mm / 568 dots |
| G Series | G11, G15, G18, G18 Pro | 193 | 25mm / 190 dots |
| TP76 Series | TP76I, TP76I Pro | 305 | 76mm / 912 dots |
| TP80 Series | TP80A, TP80A Pro | 305 | 80mm / 960 dots |
| TP86 Series | TP86A, TP86A Pro | 305 | 86mm / 1032 dots |
| SP650 | SP650 | 203 | 48mm / 384 dots |

BT-only models (E10, E11, E12, E16) are also supported via the T50 driver.
Katasymbol-branded equivalents (e.g. M50 Pro) work as their Supvan counterparts.

Supported transports:

- **Bluetooth** — RFCOMM (`btrfcomm://` scheme), auto-discovered via BlueZ D-Bus
- **USB HID** — hidraw (`usbhid://` scheme), auto-discovered via sysfs

Model data is defined in `data/models.toml` and can be extended without
recompilation.

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

### PAPPL version compatibility

`pappl-sys` requires **PAPPL ≥ 1.0** at build time (a hard pkg-config check).
Build behaviour vs the linked PAPPL version:

| Linked PAPPL | Builds? | USB hot-plug auto-add | BT auto-add | Notes |
|---|---|---|---|---|
| `≥ 1.4` | ✅ | ✅ on service start | ✅ | Recommended. Uses `papplSystemCreatePrinters`. |
| `1.0 – 1.3.x` | ✅ | ⚠ manual once | ✅ | Adds a one-line warning to the log on startup. `papplSystemCreatePrinters` isn't in the library; persisted printers reload normally, but a *newly* plugged USB device needs to be added once via the web UI (`http://localhost:8631/`) or `lpadmin`. After that it's persisted and works on every subsequent run. |
| `< 1.0` | ❌ | — | — | pkg-config fails the build with a clear version error. |

Distro PAPPL versions (as of writing) — pick your install path:

- **PAPPL 1.4 (full auto-add):** Ubuntu 25.10+, Fedora 40+, Arch (current).
- **PAPPL 1.3 (manual one-time add):** Debian 12/13/sid, Ubuntu 24.04 LTS, Linux Mint 22.x.
- **PAPPL 1.0 (manual one-time add):** Debian 11 (oldoldstable), Ubuntu 22.04 LTS, Linux Mint 21.x.
- **No system PAPPL (RHEL / Alma / Rocky):** build PAPPL yourself, or use `build-deb.sh` which ships PAPPL 1.4.9 alongside the app.

On a distro with only PAPPL 1.3, you'll still get correct printing and the same protocol behaviour as PAPPL 1.4 — the only user-visible difference is that the first time a *new* USB device is plugged in, you have to add it manually once. BT discovery uses BlueZ D-Bus and isn't affected.

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
