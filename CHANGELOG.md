# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/) (pre-1.0: breaking changes bump the
minor version).

## [Unreleased]

### Fixed

- **A page longer than four print buffers printed only its first block's
  worth.** Packing capped a compressed block at `bufferMAXCount = 4` buffers, a
  value read from the T80 Pro while `T50PlusPrint.multiCompression()` resisted
  decompilation. The T50 family's own encoder is in the desktop app's bundled
  JS, where the dispatch is explicit — `isT5080(getDevType(ProductId))` reaches
  webpack module `45ac`, which packs **32** buffers per block and shrinks the
  group by a quarter per retry. `isT5080` covers the T80 family too, so both
  share that encoder. A 50×80 mm page is 8 buffers: at 32 it leaves as one block
  and prints whole; at 4 it split in two and the printer laid down only the
  first, accepted and acknowledged, `completed` in CUPS, no error flag. Verified
  on a T50 Pro (`1820:207f`) over USB.
- **Contone rasters printed solid black.** The IPP layer advertises `SRGB24`,
  so a Ghostscript-rendered text label arrives as 24 bpp; only 8 bpp was
  dithered, and 24 bpp had its raw RGB copied into the 1-bit page buffer, where
  white `0xFF` became eight set dots. Adopted from
  [huntj88/supvan-cups#1](https://github.com/huntj88/supvan-cups/pull/1)
  (heeen/supvan-cups#8), verified on physical E10pro hardware by James Hunt.
- **A padded 1 bpp source sheared every row** — the page buffer was sized from
  the incoming `bytes_per_line` while `raster_to_column_major` re-reads it at
  `ceil(width/8)`. `KsJob::start` now derives its own stride. Same source.
- **Pages wider than the printhead lost one whole edge.** `center_in_printhead`
  kept the leading bytes; the media runs centred under the head, so a 50 mm
  label on the T50's 48 mm head lost 2 mm off the right instead of 1 mm off each
  side. Both branches now measure against the head's usable width, which also
  stops the 190-dot G series walking past its last column. Same source.
- **Printers with an unlisted hardware code are discoverable again.** The prefix
  table only covers codes the vendor app knows, so a newer unit was invisible;
  a firmware serial name inside Supvan's `A4:93:40` OUI is now accepted as a
  generic fallback and lands on the default family. Classic discovery gates
  pairing on SPP, treating an empty EIR UUID list as "unknown" rather than "no".
  Adopted from [huntj88/supvan-cups#3](https://github.com/huntj88/supvan-cups/pull/3).
- **A stale `models.toml` no longer kills the daemon at startup.** The on-disk
  copy under `/usr/share` is read before the embedded one, so any schema change
  stranded an installed table and panicked `models::load()`. It now logs and
  falls back to the embedded table. Same source.

### Fixed

- **Bluetooth discovery could not match any real printer name.** Printers
  advertise a firmware serial (`T0182A2507162197`), never a marketing name, so
  the old `bt_patterns` entries (`"t50"`, `"e10"`, `"e11"`, …) matched nothing
  and `discover.rs` dropped every device before pairing. `data/models.toml` now
  carries the vendor's per-model serial-prefix tables — transcribed from
  `DeviceConstants.java` and the `communication/device/*Device.java` classes —
  under a new `[[bt_names]]` section, matched as case-insensitive
  longest-prefix. Reported in supvan-cups#1 for an E10 and an E11.
- Matching is now by prefix rather than substring; a serial that happens to
  embed another model's code no longer resolves to the wrong family.
- BT and BLE queues report their actual model (`E11`, `T50M Pro`) instead of the
  placeholder `T50 Series` / `E-Series`, via the same table.
- **Every BLE write went out without a response.** `bluer`'s
  `Characteristic::write` defaults to `WriteOp::Command`, so the `with_response`
  flag in `ble.rs` was a no-op. Frames are now explicit `WriteOp::Request`
  writes, as the vendor app sends them.
- **BLE fragmentation and pacing now match the firmware.** 128-byte fragments
  with ~10 ms between them (`BasePrint.transferSplitData` splits each 512-byte
  data frame into 4 × 128), replacing the guessed 180-byte MTU fragments.
- **BLE data frames no longer wait for an ack that never comes.** The vendor
  reads a per-frame reply only over Classic Bluetooth; the new
  `SppPipe::acks_data_frames` lets the shared codec skip the wait on BLE, which
  otherwise burned the 4 s response timeout on every frame.
- **BLE connect no longer hangs for two minutes.** `BlePipe::connect` scans for
  the address when BlueZ has forgotten it, finishes the scan before connecting
  (an open discovery session is the usual reason `Connect()` never returns),
  bounds each attempt at 20 s instead of bluer's 120 s D-Bus budget, retries,
  and marks the device trusted.
- BLE discovery no longer drops a printer BlueZ has just seen for the first
  time: `Name` and `UUIDs` are read after the scan window, not at `DeviceAdded`,
  when BlueZ has not yet filled them in.

### Added

- `supvan-cli` can talk to BLE printers: a `ble://<address>` target, behind the
  crate's new `ble` feature. Previously every non-`/dev/hidraw` target was
  dialled as Classic RFCOMM with no way to select GATT.

## [0.5.1] - 2026-07-01

### Added

- Completed the command-opcode vocabulary in `cmd.rs` from the vendor Linux
  tool's source map (`com.supvan.supvaneditor` 1.1.4): `CHECK_RIB` (0x19),
  `RD_LAB_DPI`/`_24`/`_25` (0x22/0x24/0x25), `SET_PRTMODE` (0x33), `SEND_INF`
  (0x35), `SET_RFID_DATA` (0x5D), `TRANSFER` (0xF0, reserved/unused). These are
  constants only — we don't drive them yet (response parsing needs on-device
  verification). `docs/PROTOCOL.md` documents each plus the `FirmwareNeedUpgrade`
  status flag and the confirmation that `0xF0` is a reserved dot-pattern-transfer
  opcode (the live bitmap path stays `NEXT_ZIPPEDBULK` 0x5C).

## [0.5.0] - 2026-07-01

### Added

- **Firmware tooling (foundation).** `docs/FIRMWARE.md` documents the vendor's
  firmware check/download API (`api.supvan.com/api/upload/GetFirmwareFile`, no
  auth) and the T50-family flash protocol. New `data::build_firmware_frames` +
  `cmd::CMD_UPDATE_FW` (0xC6) provide the flash framing (`0xAA 0xC7` packets,
  reusing the print frame layout); the live flash is intentionally left to a
  caller (destructive, no on-device verification on T50). `scripts/supvan-fw-check.py`
  checks/downloads firmware for a given model + serial.

### Fixed

- `data::make_data_packet` checksum summed into `u16`, which could panic on a
  debug build for high-entropy (compressed) payloads whose byte-sum exceeds
  65535. Now sums in `u32` and truncates to the low 16 bits (matching the
  device); release-build checksum values are unchanged.

## [0.4.1] - 2026-07-01

### Fixed

- **BLE discovery no longer false-positives classic printers.** `ble_discover`
  now runs an LE-transport scan and requires the device to advertise a Supvan
  GATT service (`fee7`/`e0ff`/`ff00`) — the real BLE-print signature — instead
  of matching on name + OUI alone. Classic SPP printers (which expose only
  Serial Port `1101`) are correctly excluded, so a Classic-Bluetooth T50-series
  printer is no longer reported with `ble=true`.

## [0.4.0] - 2026-07-01

### Changed

- **BLE GATT support is now enabled by default** in `supvan-printer-app` (the
  `ble` feature is in the default set). Default builds pull `bluer` and need
  BlueZ build deps; use `--no-default-features` for a BlueZ-free build. The
  `supvan-proto` library and standalone `supvan-cli` keep BLE opt-in. The live
  BLE device path remains unverified pending E11/E12 hardware.

## [0.3.0] - 2026-06-26

Async transport stack + a feature-gated BLE GATT transport for BLE-only
printers (E11/E12-class). Verified end-to-end on a T50M Pro over USB and
Bluetooth-classic; the BLE path is implemented to the vendor spec but
unverified against hardware.

### Added

- `supvan-cli feed <target>` — advances one blank label via the `PAPER_SKIP`
  (0x2E) command (`Printer::paper_skip`).
- **BLE GATT transport** for BLE-only printers (E11/E12-class), behind the
  off-by-default `ble` feature (pulls `bluer`). BLE reuses the shared SPP codec —
  same 16-byte framing over GATT notify/write characteristics, with the vendor's
  service/characteristic auto-detect and byte-7 response correlation. Discovery
  scans for `^[TGD]\d{2}` advertisers in OUI `A4:93:40` and folds them into the
  unified `supvan://` device (USB → BT → BLE fallback). **Unverified against
  hardware** — we own no BLE printer; an E11/E12 reporter must validate it.

### Changed (breaking)

- **Transport stack is now async.** `Transport`, the new `SppPipe`/`SppCodec`
  split, and `Printer` are async (`async-trait`); blocking RFCOMM/HID FFI runs
  via `tokio::task::block_in_place`. This lets a natively-async BLE transport
  share one codec. Requires `ipp-printer-app` 0.8.0 (its `DeviceBackend`/
  `RasterDriver`/`PrintJobFn` callbacks went async; `list` now returns
  `Vec<DiscoveredDevice>`).
- Dropped the dead `Transport::raw_fd`; folded the `NEXT_ZIPPEDBULK` header
  encoding into `Transport::send_bulk_header` (was a `use_socket_io` branch).

## [0.2.0] - 2026-06-24

A cleanup, correctness, and modernization pass across all three crates
(`supvan-proto`, `supvan-app`, `supvan-cli`).

### Changed (breaking)

- **CLI: `target` is now a required positional argument** on `probe`, `material`,
  and `test-print`. The hardcoded developer Bluetooth address default was removed.
- **CLI returns proper process exit codes**: commands return `Result` and `main`
  maps failures to a single error message + exit code 1, replacing scattered
  `process::exit(1)` calls.
- **Workspace migrated to Rust edition 2024** (`resolver = "3"`); adopted let
  chains in the print/poll paths.

### Fixed

- **Reconciled the printer-status → IPP `printer-state-reasons` mapping.** Two
  divergent copies (`failure_from_status` vs `KsDevice::status`) disagreed on
  `ribbon_rw_error`, `ribbon_end`, and `head_temp_high`; they now share one
  `reasons_from_status()`, so live polling and job-failure reporting agree.
- **Print-completion timeout no longer reports success.** `print_compressed` now
  returns `Err(Error::Timeout)` instead of `Ok(())` when the 30 s completion poll
  expires; `KsJob::end` warns on timeout instead of falling through silently.
- **CLI `probe` no longer swallows transport errors** — failed queries are
  surfaced instead of being dropped by `if let Ok(Some(_))` ladders.

### Removed

- Write-only `LAST_PRINT_TIME` tracking mechanism (stored, never read).
- Unused `CMD_PAPER_SKIP` / `CMD_SET_RFID_DATA` command constants.
- Unused `log` dependency from `supvan-cli`.
- Always-empty `JobManifest.printer_name` field.
- Misleading "BCD" decode branch in the `material_probe` example.
- Tightened over-broad `pub` visibility to `pub(crate)`/private.

### Internal

- Extracted shared helpers, removing duplicated logic: `decode_status_bits`
  (BT/USB status decode), `check_header` (BT response guards), `decompress_lzma`
  (real `pub fn`, was open-coded in tests), `Printer::open_usb` / `open_bt` /
  `open_target` (collapsed five transport-construction sites), `device::open_uri`
  (one scheme dispatch for three call sites), and `dial_and_cache`.
- Named previously-bare constants (frame offsets, poll budgets, density formula,
  chunk/stride sizes, default media) and idiomatized manual loops with iterators,
  combinators, and `to_le_bytes`/`from_le_bytes`.

### Dependencies

- `cargo update`: 46 in-range patch/minor lockfile bumps.
- `toml` 0.8 → 1.0.
- Migrated `xz2` 0.1 → `liblzma` 0.4 (the maintained continuation of the same
  liblzma bindings; identical API, built from source via `cc`).

## [0.1.0]

- Initial native-Rust Supvan T50 label-printer stack: `supvan-proto` (BT/USB HID
  protocol), `supvan-app` (IPP Everywhere printer application bridging CUPS), and
  `supvan-cli` (direct diagnostic tool).
