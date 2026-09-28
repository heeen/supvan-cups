<!-- E11-FORK-NOTICE -->

# supvan-cups E11 BLE support fork

This repository is a development fork of
[heeen/supvan-cups](https://github.com/heeen/supvan-cups), created by
Florian Hänel (`heeen`).

The original project, architecture, protocol implementation, IPP/CUPS
integration, diagnostic tooling and the great majority of this codebase are
Florian Hänel's work. This fork remains under the original MIT licence and
retains the upstream licence and commit history.

Upstream project:

- https://github.com/heeen/supvan-cups
- Author: Florian Hänel (`heeen`)
- Licence: MIT

## Purpose of this fork

The immediate purpose of this fork is to add and experimentally verify working
Bluetooth Low Energy printing support for the Supvan/Katasymbol E11 label
printer.

Upstream already contained BLE transport support and documented the E11/E12
class as BLE-only hardware, but the live E-series print flow had not been
verified on physical E11 hardware. The E11 also identifies itself internally as
`G15`, which makes model-based E-series selection unreliable.

Development and testing for this fork has been performed against a physical
Supvan/Katasymbol E11 with:

- BLE advertising-name prefix: `T0183...`
- GATT service: `0000fee7-0000-1000-8000-00805f9b34fb`
- characteristic: `0000fec1-0000-1000-8000-00805f9b34fb`
- reported device name: `G15`
- reported material: 15 mm × 50 mm

## Additional prior work credited

The E-series implementation also draws on experimental work in:

- https://github.com/huntj88/supvan-cups
- branch: `feat/e-series-support`

That work provided E10pro observations derived from a vendor-app Bluetooth HCI
capture, including important differences between the normal T-series print
path and the E-series path.

Those E10pro observations were treated as hypotheses for the E11 and then
tested individually against physical E11 hardware rather than assumed to be
universally correct.

## Changes in this fork

Relative to the upstream baseline used for this work
(`9fd7af09580c3e710202e5471e77e0b88db0e3bb`), this fork currently adds:

- BLE GATT writes using write-without-response where required by the E11's
  characteristic.
- An explicit `PrintProfile` abstraction separating T-series and E-series
  protocol behaviour.
- E-series print-buffer geometry and buffer construction.
- E-series 4000-byte buffers with profile-specific payload limits.
- Per-buffer LZMA compression for E-series printing.
- E-series `BUF_FULL(0, 0)` behaviour.
- Profile-specific pre-start and post-start commands derived from E-series
  observations.
- Profile-aware test-pattern geometry.
- An explicit `supvan-cli test-print --e-series` option so E-series mode can be
  selected even though the E11 reports its internal device name as `G15`.
- E-series-specific interpretation of the `ribbon_end` status bit during
  thermal printing.
- Automatic selection of the E-series profile for discovered E11 BLE devices,
  without relying on the printer's misleading internal `G15` device name.
- A provisional E11 model family with 203 dpi geometry and the physically
  verified 15 mm x 50 mm media definition.
- Profile-aware IPP print-buffer construction and per-buffer E-series LZMA
  compression.
- Profile-aware IPP status and error handling, including non-fatal E-series
  `ribbon_end` behaviour.
- E-series media-status handling that does not treat the E11's observed
  `remaining=0` material counter alone as `media-empty`.
- End-to-end CUPS/IPP printing support verified from a normal desktop
  application through BLE to the physical E11.

## Physical E11 verification

The following has now been demonstrated on a physical E11 over BLE:

1. Device connection and GATT discovery.
2. Status, device-name, firmware and material queries.
3. Entry into the E-series print sequence.
4. Transfer of multiple compressed print buffers.
5. Correct use of `BUF_FULL(0, 0)` without the buffer deadlock seen with the
   T-series flow.
6. Successful completion of a two-buffer thermal test print.
7. Coherent printed raster geometry rather than corrupted or random data.
8. Normal print completion despite the E11 asserting the status bit decoded as
   `ribbon_end` during printing.
9. E11 discovery and model selection through the IPP printer application.
10. DNS-SD/Avahi advertisement as a Supvan E11 IPP Everywhere printer.
11. CUPS submission of a real print job through the IPP service.
12. Correct reporting of loaded 15 mm x 50 mm media without a false
    `media-empty` condition.
13. Physical printing from LibreOffice Writer through a permanent driverless
    CUPS queue.
14. Standard IPP Landscape orientation producing the expected left-to-right
    printing along the label ribbon.

The successful protocol test used two separately compressed E-series buffers
and completed with the printer returning to an idle state. Subsequent desktop
tests verified the complete application -> CUPS -> IPP -> BLE -> E11 path.

## Verified CUPS and desktop printing

The E11 has also been verified end-to-end through the normal Linux desktop
printing stack:

1. `supvan-printer-app` runs as the user-scoped systemd service.
2. The E11 is advertised over DNS-SD/Avahi as an IPP Everywhere printer.
3. CUPS discovers the printer and can submit jobs through the IPP service.
4. A permanent driverless CUPS queue created with `-m everywhere` exposes the
   E11 media definitions correctly to desktop applications.
5. LibreOffice Writer has successfully printed formatted labels through CUPS
   to the physical E11 over BLE.
6. Standard IPP landscape orientation produces the expected left-to-right
   printing along the label ribbon.

On the tested Ubuntu desktop, CUPS' temporary DNS-SD queue was sufficient for
printing, but LibreOffice Writer exposed only generic standard paper sizes
through that queue. Creating a permanent driverless CUPS queue allowed
LibreOffice to obtain the E11's advertised custom media definitions correctly.

The queue can be created with:

```sh
sudo lpadmin \
    -p Supvan_E11 \
    -E \
    -v "ipp://localhost:8631/ipp/print/<name>" \
    -m everywhere
```

For the tested E11 configuration, CUPS then exposes the available media,
including the physically verified `15x50mm` size:

```text
PageSize/Media Size: 10x10mm.Borderless *15x50mm 50x120mm Custom.WIDTHxHEIGHT
```

The permanent queue is therefore a desktop-client integration workaround; it
does not change the Supvan protocol, raster implementation or physical media
handling.

A working LibreOffice Writer configuration for the verified media is:

- document page: 5.00 cm x 1.50 cm;
- printer: `Supvan_E11`;
- media: `15x50mm`;
- orientation: Landscape.

Fonts, point sizes, bold, italic, alignment, graphics and other document
formatting remain normal application/print-stack functions. The Supvan driver
receives the resulting raster; it does not need to implement those formatting
features itself.

The E11 material query reports `remaining=0` on the tested loaded roll even
though the printer reports neither `label_end` nor `label_not_installed`.
E-series status handling therefore does not treat that zero counter alone as
`media-empty`; the physical media-status bits remain authoritative.

## Current experimental assumptions

Some E11 parameters are not yet considered confirmed.

In particular:

- The current E-series printhead geometry is 96 dots / 12 bytes per line.
- That value originates from E10pro work and has been shown to produce a valid
  E11 print, but it has not yet been established as the E11's actual full
  printable width.
- The physical E11 uses 15 mm media, while the current experimental raster is
  limited to approximately 12 mm.
- The meaning of all raw E11 status bits is not yet completely decoded.
- Behaviour shared across E10, E11, E12 and E16 must not be assumed solely from
  one tested E11 and one captured E10pro.

These items should remain explicit experimental parameters until confirmed by
measurement or additional captures.

## Development approach

Changes in this fork are being made incrementally against physical hardware:

- preserve upstream T-series behaviour;
- isolate E-series differences behind profile-specific logic;
- change one protocol assumption at a time;
- retain raw status information in diagnostic traces;
- compile and run the existing test suite after each change;
- commit known-working milestones before further experiments.

The objective is to produce E11 support without obscuring or regressing the
original upstream implementation.

---

## Original upstream README

The remainder of this file is Florian Hänel's upstream README.

# Supvan label printer driver

A pure-Rust **IPP Everywhere** printer application for Supvan T-series thermal
label printers (and Katasymbol-branded equivalents), plus a command-line
diagnostic tool. It runs a local IPP server that CUPS — and any AirPrint /
IPP-Everywhere client — can print to driverlessly over USB or Bluetooth.

- **Full IPP Everywhere conformance** — `ipptool ipp-everywhere.test` passes
  32/0 (see [docs/CONFORMANCE.md](docs/CONFORMANCE.md)).
- **Formats**: PWG/CUPS raster and `image/jpeg` (decoded in-process).
- **USB + Bluetooth**, unified into one logical printer per device.
- **CUPS-managed** — a self-contained IPP Everywhere service: it advertises over
  DNS-SD and CUPS makes an on-demand queue (no queue to install or manage).
- **Holds jobs when the device is offline** (or jammed) and prints them on
  recovery, instead of dropping them; drops out of print dialogs when off.

Built on [`ipp-printer-app`](https://crates.io/crates/ipp-printer-app), this
project's own generic IPP-Everywhere framework. The printer protocol was
reverse-engineered from the Katasymbol Android app (v1.4.20); the repo is
`github.com/heeen/supvan-cups` (the working tree is named `katasymbol`).

## Supported printers

All USB models use VID `0x1820`; Bluetooth and USB are auto-discovered.

| Family | Models | DPI | Printhead |
|--------|--------|-----|-----------|
| T50 Series | T50M, T50M Pro, T50M Plus, T50s, T50s Pro | 203 | 48 mm / 384 dots |
| T80 Series | T80M, T80M Pro | 201 | 72 mm / 568 dots |
| G Series | G11, G15, G18, G18 Pro | 193 | 25 mm / 190 dots |
| TP76 Series | TP76I, TP76I Pro | 305 | 76 mm / 912 dots |
| TP80 Series | TP80A, TP80A Pro | 305 | 80 mm / 960 dots |
| TP86 Series | TP86A, TP86A Pro | 305 | 86 mm / 1032 dots |
| SP650 | SP650 | 203 | 48 mm / 384 dots |

Bluetooth-only models (E10, E11, E12, E16) run on the T50 driver, as do
Katasymbol-branded equivalents. The model registry lives in
[`data/models.toml`](data/models.toml) — it is compiled into the binary as a
fallback and can be overridden at runtime with `SUPVAN_MODELS` (no recompile).

## How it works

```
discovery          IPP server (ipp-printer-app)         device (supvan-proto)
USB + BT  ──┐                                       ┌── column-major 1-bit pack
            ├─► supvan://<id> ─► Print-Job ─► print_job ─► LZMA ─► USB/BT transfer
mock://  ───┘    (mock://ID)        │  └─ image/jpeg ─► run_jpeg_job (decode→fit→dither)
                                    └──── PWG/CUPS raster ─► run_cups_raster_job
```

- **Discovery** unifies a printer's USB (hidraw) and Bluetooth (RFCOMM)
  interfaces into a single `supvan://<id>` device; `SUPVAN_MOCK=1` substitutes a
  synthetic `mock://` device.
- The **IPP server** (from `ipp-printer-app`) receives jobs; the `print_job`
  callback branches on `document-format` → `run_jpeg_job` (JPEG: decode →
  contain-fit onto the loaded label → dither) or `run_cups_raster_job`
  (PWG/CUPS raster), both feeding the `supvan-proto` pack → LZMA → transfer
  pipeline.
- We advertise over **DNS-SD** and let CUPS create a temporary on-demand queue
  (the AirPrint model) — no queue of our own. A co-resident `cups-browsed`
  should run with `OnlyUnsupportedByCUPS Yes` so it defers to CUPS rather than
  building a duplicate `implicitclass://` queue (see [docs/DEPLOY.md](docs/DEPLOY.md)).
- A **status poller** surfaces the loaded roll (`media-ready` / `media-col-ready`),
  a labels-remaining supply gauge (`printer-supply`), and error reasons — and
  drives the offline/jam behavior: when the device can't print, jobs are held
  and retried, the printer reports `stopped`, and its advert is withdrawn.

## Workspace layout

| Crate | Purpose |
|-------|---------|
| `crates/supvan-proto` | Wire protocol: USB-HID + BT-RFCOMM transports, commands, status/material parsing, bitmap packing, LZMA compression. No IPP knowledge. |
| `crates/supvan-app` | The printer application binary `supvan-printer-app`. |
| `crates/supvan-cli` | The `supvan-cli` diagnostic tool. |

The IPP/HTTP layer is the external crate **`ipp-printer-app`** (`= "0.7"`, on
crates.io) — this repo's own device-agnostic framework, not a workspace member.

## Install & run

The app needs no privileges (unprivileged port 8631; it owns no CUPS queue), so
the default is a **user-scoped** install — no sudo:

```sh
make deploy      # cargo install → ~/.cargo/bin + a user systemd unit, then start
```

Re-run `make deploy` after any change. `make uninstall-user` reverses it. For a
system-wide (FHS) install instead, `sudo make install`. Full details and the
`cups-browsed` story are in **[docs/DEPLOY.md](docs/DEPLOY.md)**; a manual CUPS
acceptance walkthrough is in [docs/CUPS_ACCEPTANCE.md](docs/CUPS_ACCEPTANCE.md).

Build prerequisites (Debian/Ubuntu): `sudo apt install pkg-config libdbus-1-dev`,
plus `bluez` for Bluetooth and `cups` at runtime. Run `make help` for all
targets (`build`, `test`, `clippy`, `lint`, `run`, …).

To run it directly without installing:

```sh
make run                          # cargo run -p supvan-app (add SUPVAN_MOCK=1 for no hardware)
# index + IPP server at http://localhost:8631/
```

## CLI tool

`supvan-cli` talks to a printer directly (bypassing CUPS) for diagnostics. Pass
a Bluetooth address or a `/dev/hidrawN` path:

```sh
supvan-cli discover                          # scan for Supvan Bluetooth devices
supvan-cli probe AA:BB:CC:DD:EE:FF           # device/status/material/version
supvan-cli material /dev/hidraw7             # loaded label + RFID + remaining
supvan-cli test-print /dev/hidraw7 --density 4
```

## Testing

```sh
cargo test --workspace        # unit + integration tests, no hardware
```

A Docker-based end-to-end test boots the app under CUPS + `cups-browsed` +
avahi and exercises discovery, `cups-browsed` coexistence, PWG-raster and JPEG
print round-trips, offline/jam hold-and-retry, and the status lifecycle — see
`ci/run-integration-test.sh`
(run by GitHub Actions alongside `cargo test`/`clippy`).

For label-free local testing, run with `SUPVAN_MOCK=1`: every page is written
as a PBM (plus a JSON manifest) under `$XDG_RUNTIME_DIR/supvan-mock/` (or
`SUPVAN_DUMP_DIR`) instead of reaching hardware. To exercise the IPP error
surface without a physical fault, layer on `SUPVAN_MOCK_FAIL=media-empty`
(single shot) or `SUPVAN_MOCK_STICKY=cover-open SUPVAN_MOCK_RECOVER_AFTER_MS=10000`
(a sticky `printer-state-reasons` that clears after 10 s). Reason tokens:
`media-empty`, `label-not-installed`, `media-jam`, `label-rw-error`,
`label-mode-error`, `ribbon-rw-error`, `ribbon-end`, `media-needed`,
`cover-open`, `head-temp-high`, `other`.

## Environment variables

| Variable | Description |
|----------|-------------|
| `SUPVAN_HOST` | Bind address (default `0.0.0.0`) |
| `SUPVAN_PORT` | IPP/HTTP port (default `8631`) |
| `SUPVAN_MODELS` | Override path to `models.toml` (else the embedded copy) |
| `SUPVAN_MOCK` | `1` runs a synthetic printer (no hardware) |
| `SUPVAN_DUMP_DIR` | Directory for debug page dumps |
| `RUST_LOG` | Log level (`debug`, `info`, `warn`, `error`) |
| `IPP_PRINTER_APP_POLL_SECS` | Status-poll cadence in seconds (default `30`) |
| `SUPVAN_MOCK_DELAY_MS` | Mock transfer delay per page (default `0`) |
| `SUPVAN_MOCK_FAIL` | Mock single-shot failure reasons (token list above) |
| `SUPVAN_MOCK_FAIL_REPEAT` | `1` re-arms `SUPVAN_MOCK_FAIL` after each use |
| `SUPVAN_MOCK_STICKY` | Mock sticky `printer-state-reasons` (same tokens) |
| `SUPVAN_MOCK_RECOVER_AFTER_MS` | Sticky reasons auto-clear after N ms |

## Documentation

- [docs/DEPLOY.md](docs/DEPLOY.md) — install, the systemd units, `cups-browsed` coexistence.
- [docs/CONFORMANCE.md](docs/CONFORMANCE.md) — the IPP Everywhere `ipptool` audit.
- [docs/CUPS_ACCEPTANCE.md](docs/CUPS_ACCEPTANCE.md) — manual CUPS acceptance walkthrough.
- [docs/PROTOCOL.md](docs/PROTOCOL.md) — the reverse-engineered Supvan wire protocol.

## License

MIT — see [LICENSE](LICENSE).
