# Supvan Printer Driver

Linux printer driver for Supvan thermal label printers. Provides a
**Rust IPP Everywhere** printer application (`ipp-printer-app` + Axum on port 8631)
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
| `ipp-printer-app` | Generic IPP Everywhere framework (`ipp` + Axum + `print_raster`, optional mDNS via `mdns-sd`). Device-agnostic; a consumer crate plugs in a `DeviceBackend` + `RasterDriver`. |
| `supvan-app` | Printer application binary (`supvan-printer-app`) |
| `supvan-cli` | Command-line diagnostic tool (`supvan-cli`) |

## Prerequisites

Rust toolchain (edition 2021) and:

```sh
# Debian / Ubuntu
sudo apt install libcups2-dev pkg-config libdbus-1-dev bluez
```

No `libpappl-dev` is required — the IPP stack is pure Rust.

CUPS driverless printing: see [docs/CUPS_ACCEPTANCE.md](docs/CUPS_ACCEPTANCE.md).

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

## Testing

Run the Rust unit and integration tests (all workspace crates, no hardware):

```sh
cargo test --workspace
```

End-to-end printing against a real printer is exercised manually via
`supvan-cli` (see below) or through CUPS ([acceptance checklist](docs/CUPS_ACCEPTANCE.md)).
IPP golden request fixtures: `tests/fixtures/ipp/` (see `scripts/capture_ipp_golden.sh`).

## Usage

### Printer Application

The printer application runs an IPP server with a simple index page on port **8631**.
It auto-discovers printers via BlueZ D-Bus (Bluetooth) and sysfs (USB HID),
and registers them as IPP Everywhere printers (`ipp://localhost:8631/ipp/print/<name>`).
When both transports are available, USB is preferred.

```sh
supvan-printer-app                # start server (default 0.0.0.0:8631)
# Index: http://localhost:8631/
```

**First-install step** — register the queue with CUPS:

```sh
lpadmin -p QUEUE -E -v ipp://localhost:8631/ipp/print/PRINTER -m everywhere
```

You only do this once per install; CUPS persists the queue across reboots.

For automatic discovery (no manual `lpadmin`), enable `cups-browsed`:

```sh
sudo systemctl enable --now cups-browsed
```

The app advertises printers over mDNS (`_ipp._tcp.local.`) by default, and
`cups-browsed` picks them up within ~10 s. Disable mDNS at build time with
`--no-default-features` on `ipp-printer-app` if you don't want it (e.g. for
USB-only embedded targets).

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
| `SUPVAN_HOST` | Bind address (default: `0.0.0.0`) |
| `SUPVAN_PORT` | IPP HTTP port (default: `8631`) |
| `IPP_PRINTER_APP_POLL_SECS` | Status-poll cadence in seconds (default: `30`) |

## License

MIT
