# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/) (pre-1.0: breaking changes bump the
minor version).

## [Unreleased]

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
