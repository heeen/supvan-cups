# Supvan T-series printer protocol

Reverse-engineered notes on the wire protocol the Supvan T50 / T50M Pro
label printers speak. Two transports — Bluetooth RFCOMM (SPP) and USB HID
— share the same command codes but use **different framings and
different response sizes per command**. That asymmetry is the single
biggest gotcha when working with this hardware; this document is the
crib sheet for it.

Sources of ground truth:

- The vendor Android SDK and the vendor's Electron desktop app, both
  reverse-engineered in 2026-05 while bringing up the original C-PAPPL
  backend. Where a field comment says "from Electron app" or
  "byteToString(A,11,21)" it's a direct port of that source.
- `supvan-proto/src/{cmd,status,bt_transport,usb_transport}.rs` — the
  Rust implementation. Always more authoritative than this document.
- Live captures from the working katasymbol deployment (the BT side
  was self-validated by ipptool round-trips; the USB side was decoded
  from `cargo test` fixtures and `cargo run` traces).

## Transports

|                       | BT (RFCOMM, SPP)                                | USB HID                                              |
|-----------------------|-------------------------------------------------|------------------------------------------------------|
| Underlying channel    | Bluetooth Classic SPP, RFCOMM channel auto-detected via `RfcommSocket::connect_default` | Hidraw (`/dev/hidrawN`) on a Supvan VID `0x1820` device |
| Framing magic         | `7E 5A` header                                  | `C0 40` header                                       |
| Command size sent     | 16 bytes (always)                               | 8 bytes (most cmds), 10 bytes for two-param cmds      |
| Param byte order      | Little-endian at frame[12..14]                  | Big-endian at frame[2..4] (NB the swap)              |
| Response size         | Variable, status-frame-prefixed                  | **Fixed 8 bytes** for most commands; **64 bytes** for `RETURN_MAT` |
| Carries device name in `RD_DEV_NAME`? | yes, ASCII at frame[22..]              | **no** — the 8-byte response has no string slot     |
| Carries device serial in `RETURN_MAT`? | yes, BCD bytes at frame[51..57]       | **yes**, ASCII null-terminated at offset 40         |

Because the USB response is fixed-size for everything except
`RETURN_MAT`, several BT-only data items (firmware version, device name,
extended version) are stubbed to `None` on the USB transport in
`usb_transport.rs::parse_*_response`. That isn't a bug; the firmware
genuinely can't encode them in 8 bytes.

## BT frame format

### Command frame (16 bytes, sent to printer)

```
[0]  7E             magic1
[1]  5A             magic2
[2]  0C             payload-length low (= 12)
[3]  00             payload-length high
[4]  10             PROTO_ID
[5]  01             PROTO_VER
[6]  AA             marker
[7]  CMD            command byte (see table below)
[8]  chk_lo         checksum low  ⎫
[9]  chk_hi         checksum high ⎭ = LE sum of bytes [10..16]
[10] 00             reserved
[11] 01             DATA_TYPE (?)
[12] param_lo       parameter, little-endian
[13] param_hi
[14] block_lo       block_count for start-trans, else 0
[15] block_hi
```

Built by `cmd::make_cmd(cmd, param)` and `cmd::make_cmd_start_trans(cmd, block_size, block_count)`.

### Response frame (variable size)

```
[0]  7E             magic1
[1]  5A             magic2
[2]  len_lo         payload-length low
[3]  len_hi         payload-length high
[4]  10             PROTO_ID
[5]  03             reply marker (vs 01 on request)
[6]  55             reply marker
[7]  CMD            command byte being acknowledged
[8..9]              checksum
[10..21]            command-specific metadata; for status responses
                     this is the registered state described below
[22..]              command-specific payload (string / material / …)
```

The first 8 bytes are a "header" — magic + reply markers + echoed command.
Validation happens in `status::validate_response(data, expected_cmd)`.

## USB HID frame format

### Command frame (8 or 10 bytes; written as a 64-byte HID report)

```
[0]  C0             USB_MAGIC1
[1]  40             USB_MAGIC2
[2]  param_hi       parameter, **big-endian** (opposite of BT)
[3]  param_lo
[4]  CMD            command byte
[5]  00             reserved
[6]  08             reserved (looks like a length hint, always 0x08)
[7]  00             reserved
[8]  param2_hi      (only the 10-byte `send_cmd_two` form)
[9]  param2_lo
```

Built by `usb_transport::UsbHidTransport::make_usb_cmd(cmd, param)` and
`make_usb_cmd_two(cmd, param1, param2)`. The HID write transparently
right-pads to 64 bytes (`HID_REPORT_SIZE`).

### Status response frame (8 bytes)

This is the only USB response size for everything *except*
`RETURN_MAT`. It carries no string payload — there's nowhere to put one.

```
[0]  echo / length indicator (varies by command; not the command byte)
[1]  MSTA low   — same bits as BT byte 14
[2]  MSTA high  — same bits as BT byte 15
[3]  FSTA low   — same bits as BT byte 16
[4]  FSTA high  — same bits as BT byte 17
[5]  print count low
[6]  print count high
[7]  reserved
```

Parsed by `usb_transport::UsbHidTransport::parse_usb_status(resp)`.
The bit assignments are the **same** as BT (see PrinterStatus table
below); the frame just packs them at different offsets.

### Material response frame (64 bytes)

Returned by `RETURN_MAT (0x30)` only. The HID descriptor evidently
declares a second, larger feature/output report just for material data.

```
[0]        length / type indicator
[1..8]     status bytes (same shape as INQUIRY_STA response)
[19]       width_mm   (label width, integer millimetres)
[20]       height_mm  (label height)
[21]       gap_mm     (inter-label gap)
[22]       label_type (see vendor docs; not enumerated here)
[31..32]   SN low/high (u16 LE) — vendor "label SN" counter; NOT the
            device serial.
[40..]     device serial as ASCII, null-terminated.
            For the T50M Pro reference unit this is "T0117A2410211517",
            the same string the firmware broadcasts as the BlueZ Name.
```

Parsed by `usb_transport::UsbHidTransport::parse_usb_material(resp)`.

The note from the original Electron app — `byteToString(A,11,21)` —
hinted at a device serial at offsets 11..21 (10 bytes BCD); we don't
currently extract that path because the offset-40 ASCII string is
sufficient and self-validating. **TODO**: cross-check whether the
BCD bytes at 11..21 match the ASCII at 40+. If they do it's a
redundant encoding; if not, one of them is the *device* serial and
the other is the *label* serial.

## Command reference

The leading-zero byte is `cmd::CMD_*` in `cmd.rs`. Italicised entries are
output-only (no parsed response).

| Code | Name                | Direction       | BT response shape         | USB response shape           | Parser                             |
|------|---------------------|-----------------|---------------------------|------------------------------|------------------------------------|
| 0x10 | BUF_FULL            | host → device   | *control / flow only*     | *control / flow only*        | —                                  |
| 0x11 | INQUIRY_STA         | host ↔ device   | 20-byte status frame      | 8-byte status frame          | `parse_status` / `parse_usb_status` |
| 0x12 | CHECK_DEVICE        | host ↔ device   | 8-byte ack (non-empty)    | 8-byte ack (non-empty)       | `validate_response`                |
| 0x13 | START_PRINT         | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x14 | STOP_PRINT          | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x16 | RD_DEV_NAME         | host ↔ device   | ≥22-byte frame + ASCII    | **stub** (8-byte; no string) | `parse_device_name`; USB returns None |
| 0x17 | READ_REV            | host ↔ device   | ≥25-byte frame + ASCII    | **stub** (8-byte)            | `parse_version`                    |
| 0x2E | PAPER_SKIP          | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x30 | RETURN_MAT          | host ↔ device   | ≥57-byte material frame   | **64-byte** material frame   | `parse_material` / `parse_usb_material` |
| 0x5C | NEXT_ZIPPEDBULK     | host → device   | uses `make_cmd_start_trans`; signals next block of zipped raster | same | — |
| 0x5D | SET_RFID_DATA       | host → device   | not yet exercised         | not yet exercised            | — |
| 0xC5 | READ_FWVER          | host ↔ device   | ≥23-byte frame; firmware byte at [22] | **stub** (8-byte)  | `parse_firmware_version`           |

### Print pipeline glue

The above list is the per-command vocabulary. A real print job uses
them in this order (BT, simplified):

1. `CHECK_DEVICE` to confirm liveness.
2. `INQUIRY_STA` to verify no error flags before committing.
3. `START_PRINT` with the speed/darkness param.
4. For each compressed-raster block:
   - `NEXT_ZIPPEDBULK` (start-trans framing carrying `block_size`,
     `block_count`).
   - `BUF_FULL` (start-trans framing carrying `compressed_len`,
     `speed`) → upload raw bytes with the BT transport's
     `send_bulk_data` per-packet ack loop.
5. Poll `INQUIRY_STA.printing` until it clears.
6. `STOP_PRINT`.

The KsJob raster pipeline in `supvan-app/src/job.rs::transfer_page`
implements exactly this sequence.

## PrinterStatus bit layout

Same bit assignments on both transports. Differs only in *where* the
bytes live inside the response frame (BT at offsets 14..20, USB at
offsets 1..7).

| Field                | Reg byte | Bit mask | Source     |
|----------------------|----------|----------|------------|
| `buf_full`           | MSTA low (b14 / r1) | 0x01 | b0 |
| `label_rw_error`     | MSTA low | 0x02     | b0         |
| `label_end`          | MSTA low | 0x04     | b0         |
| `label_mode_error`   | MSTA low | 0x08     | b0         |
| `ribbon_rw_error`    | MSTA low | 0x10     | b0         |
| `ribbon_end`         | MSTA low | 0x20     | b0         |
| `low_battery`        | MSTA low | 0x40     | b0         |
| `device_busy`        | MSTA high (b15 / r2) | 0x04 | b1   |
| `head_temp_high`     | MSTA high | 0x08    | b1         |
| `cover_open`         | FSTA low (b16 / r3) | 0x08  | b2         |
| `insert_usb`         | FSTA low | 0x10     | b2         |
| `printing`           | FSTA low | 0x40     | b2         |
| `label_not_installed`| FSTA high (b17 / r4) | 0x01 | b3        |
| `print_count` (u16)  | b18..19  | full byte LE | resp[5..7] |

Mnemonic: **M**aster state for jam/empty/buffer, **F**lag state for
cover/operator/job state.

## MaterialInfo layout

```rust
pub struct MaterialInfo {
    pub uuid: String,          // BT only (hex-uppercase from frame[22..29])
    pub code: String,          // BT only (hex-uppercase from frame[29..37])
    pub sn: u16,               // both transports; "label SN" counter
    pub label_type: u8,        // both transports
    pub width_mm: u8,          // both transports
    pub height_mm: u8,         // both transports
    pub gap_mm: u8,            // both transports
    pub remaining: Option<u32>, // BT only (frame[43..47] LE); USB stubs None
    pub device_sn: Option<String>, // both transports; THIS is the cross-transport join key
}
```

### Cross-transport correlation

`MaterialInfo.device_sn` is the only field we've verified to carry the
same string over both transports:

- BT: parsed from BCD bytes at `frame[51..57]` (6 bytes of BCD → 12 ASCII
  digits, but the BCD parser concatenates `{byte:02}` per byte, so what we
  return is `"AABBCCDDEEFF"`-style not the printer's literal label).
- USB: parsed as null-terminated ASCII starting at `frame[40]`.

For the T50M Pro reference unit both encode the same printer's
`T0117A2410211517` serial — though after the BT BCD parser passes it
through `format!("{:02}", byte)` formatting, the BT-side value is
**not** literally the same string as the USB value. The discovery code
in `supvan-app/src/ipp_server.rs::SupvanDeviceBackend::list` uses the
BlueZ `Device1.Name` property (an independent path that *does* yield
the ASCII serial) rather than `MaterialInfo.device_sn` for the BT side.

**TODO**: align the BT `device_sn` parser to produce the same ASCII
serial as USB, so a future `MaterialInfo.device_sn`-only correlation
works cleanly without leaning on BlueZ properties.

### Remaining labels

Both transports physically carry this — the firmware updates it every
print — but only the BT parser extracts it today. The USB parser
returns `Some(None)` for `remaining`. From the comment at offset 31..32
("SN" counter), it's plausible the USB-side counter at that offset is
the same value, but the field is currently used for the label SN, not
the remaining count.

**TODO**: figure out where the USB 64-byte report carries the
remaining-label counter (probably also somewhere in 30..50). The
`remaining=None` we observe live confirms it's not at the BT offsets.

## Known gaps and open questions

| Gap | Where | Impact |
|---|---|---|
| `RD_DEV_NAME` over USB | `usb_transport::parse_device_name_response` | Can't get the printer name from a status query; have to issue `RETURN_MAT` instead. |
| `READ_REV` over USB | `parse_version_response` stub | We can't read the protocol version string over USB. Probably fine; same firmware on both ends. |
| `READ_FWVER` over USB | `parse_firmware_version_response` stub | No way to read firmware version over USB without bigger HID report. |
| `MaterialInfo.remaining` over USB | parse_usb_material returns None | Label-counter UX broken for USB-only setups. |
| BT `device_sn` BCD vs USB ASCII | `status::parse_material` | The two transports report the same physical value but in different encodings; downstream code can't naïvely string-compare. |
| `SET_RFID_DATA` (0x5D) | not exercised by any code path | We've never sent it. Firmware support unknown. |
| `BUF_FULL` (0x10) handling | request side is implemented; what the device sends back when its buffer fills mid-print isn't fully decoded. | KsJob's per-packet ack loop handles the timing but doesn't surface a typed status. |

## See also

- `crates/supvan-proto/src/cmd.rs` — command constants + frame builders.
- `crates/supvan-proto/src/status.rs` — BT response parsers + bit
  assignments.
- `crates/supvan-proto/src/bt_transport.rs` /
  `crates/supvan-proto/src/usb_transport.rs` — Transport trait impls;
  the place where per-transport quirks land.
- `crates/supvan-proto/src/printer.rs` — high-level Printer interface;
  one method per command code, returning the parsed shape.
- `crates/supvan-app/src/job.rs::transfer_page` — the real-world
  ordering of these commands for a one-page label print.
