# Supvan T-series printer protocol

Reverse-engineered notes on the wire protocol the Supvan T50 / T50M Pro
label printers speak. Two transports — Bluetooth RFCOMM (SPP) and USB HID
— share the same command codes but use **different framings and
different response sizes per command**. That asymmetry is the single
biggest gotcha when working with this hardware; this document is the
crib sheet for it.

Sources of ground truth:

- The vendor Android app (Katasymbol v1.4.20, decompiled with jadx) and the
  vendor's Electron desktop app, both reverse-engineered during initial
  bring-up. Key Android sources: `BasePrint.java` (framing/transport/status),
  `T50PlusPrint.java` (print flow + material), `BluetoothUtils.java` /
  `BLEUtils.java` (transports), `LzmaUtils.java` (compression params),
  `{MSTA,FSTA,PAGE}_REG_BITS.java` (status/page bits). Where a field comment
  says "from Electron app" or "byteToString(A,11,21)" it's a direct port of
  that source.
- `supvan-proto/src/{cmd,status,spp_pipe,rfcomm,usb_transport}.rs` — the
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
extended version) default to `None` on the `Transport` trait and only the
SPP codec parses them. That isn't a bug; the firmware genuinely can't encode
them in 8 bytes.

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

Built by `usb_transport::make_usb_cmd(cmd, param)` and
`make_usb_cmd_two(cmd, param1, param2)`. The HID write right-pads to 64
bytes and prefixes report ID `0x00` (see "hidraw needs an explicit report
ID" below).

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

Parsed by `status::decode_status_regs(&resp[1..])`.
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

Parsed by `UsbHidTransport::parse_material_response`.

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
| 0x11 | INQUIRY_STA         | host ↔ device   | 20-byte status frame      | 8-byte status frame          | `parse_status` / `decode_status_regs` |
| 0x12 | CHECK_DEVICE        | host ↔ device   | 8-byte ack (non-empty)    | 8-byte ack (non-empty)       | `validate_response`                |
| 0x13 | START_PRINT         | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x14 | STOP_PRINT          | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x16 | RD_DEV_NAME         | host ↔ device   | ≥22-byte frame + ASCII    | **stub** (8-byte; no string) | `parse_device_name`; USB returns None |
| 0x17 | READ_REV            | host ↔ device   | ≥25-byte frame + ASCII    | **stub** (8-byte)            | `parse_version`                    |
| 0x2E | PAPER_SKIP          | host → device   | 8-byte ack                | 8-byte ack                   | —                                  |
| 0x30 | RETURN_MAT          | host ↔ device   | ≥57-byte material frame   | **64-byte** material frame   | `parse_material` / USB `parse_material_response` |
| 0x5C | NEXT_ZIPPEDBULK     | host → device   | uses `make_cmd_start_trans`; signals next block of zipped raster | same | — |
| 0x5D | SET_RFID_DATA       | host → device   | 8-byte ack, then bulk payload | 8-byte ack, then bulk payload | `rfid::RfidMaterial::encode`; see below |
| 0xC5 | READ_FWVER          | host ↔ device   | ≥23-byte frame; firmware byte at [22] | **stub** (8-byte)  | `parse_firmware_version`           |
| 0xC6 | UPDATE_FW           | host → device   | firmware-transfer start (`0xAA 0xC7` packets follow) | same | `build_firmware_frames`; see docs/FIRMWARE.md |

### Extended opcodes (from the vendor Linux tool)

The vendor Linux editor (`com.supvan.supvaneditor` 1.1.4, Electron) ships an
un-minified **source map** that confirms the vocabulary above and adds the codes
below. We keep them as `cmd::CMD_*` constants but do not drive them yet —
response parsing needs on-device verification (the tool's byte offsets are for
its own USB/serial framing, not our 22-byte-header BT frames).

| Code | Name | Notes |
|------|------|-------|
| 0x19 | CHECK_RIB   | check ribbon; defined but no active call site |
| 0x22 | RD_LAB_DPI  | read label DPI (response = DPI×100 as LE u16); G/TP/MP50 |
| 0x24 / 0x25 | RD_LAB_DPI_24/25 | per-material DPI read variants (sp plugin) |
| 0x33 | SET_PRTMODE | set print mode; MP50/P70 only |
| 0x35 | SEND_INF    | set print density; MP50/P70 only |
| 0xF0 | TRANSFER    | "传输字模" (dot-pattern transfer); **reserved** — defined but never sent (the live bitmap path is `NEXT_ZIPPEDBULK` 0x5C). `字模` here is the raster dot-pattern, not typographic fonts. |

`0x11` doubles as `INQUIRY_STA` and a `FINISH_PRINT` marker; `0x14` doubles as
`STOP_PRINT` / `RESET_PRINT`.

### SET_RFID_DATA (0x5D) — synthetic material records

Genuine consumables carry an RFID tag the printer reads via `RETURN_MAT`. Stock
whose tag the printer can't read — third-party, or another brand's — returns an
all-zero material record and raises `label_rw_error`. The vendor's answer is to
synthesise the record host-side and push it over 0x5D, gated on exactly that
all-zero UUID (`MatCtrlFunc.sendRfid()`, for T50/T50S/T50Plus/T50Pro/T80Pro):

    if (UUID.indexOf("00000000") == -1)  isRFIDLabel = false   // real tag, leave it
    else                                 isRFIDLabel = true    // blank, send one

Two steps on the wire: announce the payload length under 0x5D, then bulk-write
the record. The vendor's `CMD_SET_RFID_DATA_WRITE: 999` is an internal step
marker, not a second opcode. `printer::Printer::set_rfid_data` implements this;
`supvan-cli provision` drives it.

**Record layout** (80 bytes; every field but the UUID sits at base offset 16,
matching the vendor's `databuf[N + offset]`):

| Offset | Size | Field |
|-------:|-----:|-------|
| 0  | 7  | UUID — decimal catalogue code, right-padded with `'0'` to 14 hex chars |
| 16 | 8  | MatCode (left zero for synthetic records) |
| 24 | 2  | MatSn, LE u16 |
| 26 | 1  | MatType (`PaperTypeEnum`) |
| 27 | 1  | width across the printhead, mm (≤50) |
| 28 | 1  | length along the feed, mm (≤120) |
| 29 | 1  | CustomID (0 = Supvan, 1 = 贴博士) |
| 30 | 1  | GapDistance, mm |
| 31 | 1  | TailLength, mm |
| 32 | 4  | RemainUsage (labels), LE u32 |
| 36 | 2  | **HeatTime5**, LE u16 |
| 38 | 2  | **HeatTime40**, LE u16 |
| 40 | 6  | TimeStamp (zero) |
| 48 | 16 | Cipertext (zero) |
| 64 | 2  | MinOptThreshold, LE u16 |
| 66 | 2  | MaxOptThreshold, LE u16 |
| 68 | 1  | OptIndex |
| 69 | 1  | HeatIndex |

The anti-counterfeit fields are zero because the vendor ships them that way:
`GeneralCipertext1..5()` are commented out and `Cipertext` stays `null`, which
its `deepCopy()` turns into `{}` — so `Cipertext[i]` is `undefined` and lands in
the `Uint8Array` as 0. The `Password` that `generalPassword()` computes is never
serialised. Reimplementing that crypto would be dead work.

**HeatTime5 / HeatTime40** are printhead pulse widths at 5 °C and 40 °C ambient,
which the firmware interpolates between — the only *absolute* energy control in
the protocol. Vendor presets, selected by catalogue code:

| Profile | HeatTime5 | HeatTime40 |
|---------|----------:|-----------:|
| standard label stock          | 1700 | 1200 |
| index / transparent black-mark | 1800 | 1300 |
| black-mark cardstock          | 2500 | 2000 |

### The consumable tags themselves

Read off two rolls with an NFC phone app, decoded here. Relevant because it
explains *why* 0x5D exists as the only route for third-party stock.

| | Supvan (genuine) | Niimbot (third-party) |
|---|---|---|
| UID | `1D 98 78 7F 02 10 80` | `1D FD 47 C7 18 10 80` |
| RF / chip | ISO 14443-A, NTAG213-class, 7-byte UID | identical |
| CC (page 3) | `E1 10 12 00` — NFC Forum T2, 144 bytes, RW | identical |
| lock bytes (page 2) | `00 00` — nothing locked | `80 FF` — pages 7-15 locked |

**Same tag hardware, and not genuine NXP.** Real NXP NTAG carries manufacturer
byte `0x04`; both of these start `0x1D`. Same prefix and identical capability
container on rolls from two unrelated brands points at one OEM tag supplier
behind both.

**Both are read-protected from page 4.** A full dump aborts with a read error on
page 4, which is the signature of NTAG21x `AUTH0 = 0x04` with `PROT = 1`
(protection covering reads, not just writes). Pages 0-3 read fine on any tag
regardless — they sit below `AUTH0` — so being able to read them proves nothing
about protection, and an earlier reading of this as "not auth" was wrong on
exactly that point.

So the printer's failure on a foreign roll is **password authentication**, not a
protocol mismatch and not content validation: it derives a password from the UID,
authenticates, and only its own tags match. It never reaches the point of reading
content, which is why `RETURN_MAT` returns a zeroed record rather than garbage.

**Password derivation** (`generalPassword` in `t5080imageEncodeUtils.js`): NTAG21x
`PWD`/`PACK` from `SHA-256(UID ‖ Pwkey)`, taking digest bytes `[6][5][3][1]` as
the 32-bit PWD and `[9][12]` as the 16-bit PACK. `Pwkey` is the 16-byte constant
in `RFIDMatModel`. **The whole block is commented out** in the shipped editor,
with its author's note `字节数组算法有问题` ("byte array algorithm has a
problem") — so the host never computes it and the firmware must. Treat the
formula as plausible but unconfirmed.

For the two UIDs above it yields `PWD=D3F0F5E0 PACK=37A3` (Supvan) and
`PWD=DC86DD5D PACK=8625` (Niimbot — arithmetic only; Niimbot uses its own key).

**A Niimbot tag cannot be converted into a Supvan one.** The material record is
80 bytes = pages 4-23, and pages 7-15 are locked. NTAG static lock bits are
one-way. Hence `SET_RFID_DATA` injection is the only path, which is what
`Printer::provision_material` does.

**Unverified, and it matters.** `supvan-app`'s `record_is_ours` refuses to
overwrite a roll whose 8-byte signature field (`MaterialInfo::code`) is non-zero,
on the reasoning that genuine tags carry one and records we write never do.
Confirming that needs a genuine tag's pages 4+, which needs the password above.
Until then it is an assumption, and the one place a wrong guess could overwrite a
real roll.

**If you attempt authentication, mind `AUTHLIM`** (config page `0x2A`, low 3
bits). It caps consecutive failed authentications and is one-way: exceed it and
the tag refuses auth permanently, making user memory unreadable for good. Its
value can't be checked first, since the config pages are themselves above
`AUTH0`. A couple of deliberate attempts on a sacrificial roll is reasonable;
brute-forcing is how you brick it.

### Energy control and its limits

Two knobs, in series: the material record's heat times set absolute pulse width,
and the per-buffer density (0–15) trims it.

Density is **two independent trims**, not one — `Density { black, red }`. The
Android app keeps them as `mDeepness` / `mRedDeepness` and packs them for
transport as `(black << 8) | red`, unpacking whenever the value exceeds 255
(`T50PlusPrint.java:107-112`). They land in different header fields:

| trim | header field |
|------|--------------|
| black | PAGE_REG_BITS `nodu` (byte 1, bits 2-5) |
| red   | `buf[12]` |

Because density lives in the buffer header and buffers tile the label along the
**feed** axis, energy can differ from one printhead line to the next, down to a
single-column buffer (0.125 mm at 8 dots/mm). Within a line it can differ only by
*colour*, via the two-colour mode below.
`buffer::split_into_banded_buffers` exposes the feed-axis case as a list of
`DensityBand`s.

`PaperTypeEnum` has 16 entries and none is two-colour, but that does not mean the
firmware has no colour concept — it has a dedicated mode, described next.

### Two-colour mode

Two interleaved bitplanes per printhead line, each burned at its own trim, giving
**per-dot** colour selection. Recovered from
`ImgConverter.GetBytes(int, int, int, List<Color>)` and the `i2 == 3` branch of
`T50PlusPrint`; implemented in `twocolor.rs` and selected by
`buffer::ColourMode::TwoColour`.

Wire differences from mono:

| aspect | mono | two-colour |
|--------|------|------------|
| PAGE_REG_BITS `first_cut` | 0 | **2** |
| header column count | printed columns | **printed columns × 2** |
| bytes per column | `per_line_byte` | `per_line_byte × 2` |
| max columns per buffer | `MAX_BUF_DATA / bpl` | **halved** |

Each column ships its red line immediately followed by its black line. Pixels are
sorted by luma `0.3 R + 0.59 G + 0.11 B`:

| luma | destination |
|------|-------------|
| ≥ 125 | no ink |
| 48 … 125 | plane 0 (red) |
| ≤ 48 | plane 1 (black) |

The `48` cut is placed so pure red — luma `255 × 0.3 = 76` — lands on the red
side. It is a **luminance cut, not a hue test**: any mid-grey in the 48–125 window
becomes red, so quantise to two ink colours before encoding rather than feeding an
arbitrary image.

**Availability is gated, and not every unit qualifies.**
`DeviceManager.isTwoColorDevice()` returns 2 for T80 Pro / T50 Plus, and 3 for a
T50 Pro *only* when its Bluetooth name contains neither `A` nor `B`. The shipped
app never enables the path regardless: `PrintPageData.colors` is read but never
populated. `MaterialManager.isTwoColorConsumable()` additionally requires a
catalogue code in `{5602, 5618-5621, 5686-5692}` — settable via
`supvan-cli provision --code`.

**Measured on a T50M Pro (`T0117A2410211517`, USB HID): the mode is accepted but
ignored.** The buffers are taken without error and the print completes, but a card
of vertical red/black stripes — both colours on every printhead line, which only
genuine two-plane support can produce — came out uniformly one colour. The
interleave appears to be consumed as plain mono columns, rendering each stripe at
half vertical fill (hence unusually light output). This matches
`isTwoColorDevice()` excluding this serial. The encoder is retained for the
hardware the vendor does list as capable.

**On such a unit, colour is per-label, not per-area.** Heat time is the only
control that moves it; the 0-15 density trim showed no discernible effect across
its full range at the transition heats. Measured on Niimbot two-colour stock:

| heat5:heat40 | result |
|--------------|--------|
| 300:150 | faint red |
| **500:300** | **clean red** |
| 700:450 | red, black appearing at band leading edges |
| 800:500 | red + ~50% black |
| **1700:1200** | **solid black** |

Set the pair with `supvan-cli provision --heat`, or walk a range with
`supvan-cli heat-sweep`. Darker bands at the *leading edge* of each printed run
are a printhead history-compensation artefact, not a density effect.

**No host-commanded backfeed.** `PAPER_SKIP` (0x2E) feeds forward only and the
Linux editor never sends it. The editor's 标签对齐 ("label align") button is
canvas centring (`objCenterClick` sets `scale = 1`), and `keepOnPrintPosition` is
a host-side resume index for a paused queue, not paper motion.

`Savepaper` (PAGE_REG_BITS byte 0, bit 7 — 省纸) is defined and plumbed through
`mSavePaper` in the Android app, but never actually set by either vendor tool.
**Tested and inert**: two consecutive prints with the bit set produced two
normally-advanced labels, indistinguishable from the baseline, with no error.
Exposed as `PageOptions::save_paper` and `supvan-cli test-print --save-paper` so
the negative result can be re-checked on other models. The 2-bit `Cut` field is
likewise never set by the vendor and remains untested.

So a true two-pass over one label has no path: no backfeed opcode, and no way to
suppress the advance.

### Grayscale: there is none

The raster is strictly 1bpp. Two-colour doubles the *planes*, not the bit depth,
and there is no 4bpp analogue anywhere in the format. Every intermediate tone is a
host-side halftone.

The vendor does this in `BitmapUtil` — Floyd–Steinberg, Atkinson, Bayer, Stucki,
and `convertTo16GrayWithDithering`, which quantises to 16 levels
(`GRAY_16_LUT = i * 17`) and then relies on `ImgConverter`'s threshold of 125 to
get back to 1bpp — with a `ditherSwitch` in its print-setup UI.

We halftone in `dither::Ditherer`, selected by `SUPVAN_DITHER` for the IPP app or
`--dither` on `supvan-cli gray-ramp`:

| mode | notes |
|------|-------|
| `bayer` | 4×4 ordered, stateless. **Default** — `SRGB_TO_LINEAR` compensates this hardware's dot spread |
| `floyd-steinberg` | 7/16, 3/16, 5/16, 1/16 |
| `atkinson` | spreads 6/8, higher local contrast |
| `vendor16` | the vendor pipeline reproduced: 16-level quantise + their kernel + threshold 125 |

`vendor16` deliberately skips `SRGB_TO_LINEAR` — the vendor applies no thermal
compensation, and adding ours would stop it being their pipeline. Their weights
(0.3125, 0.1875, 0.375, 0.0625) sum to 15/16 rather than 1, dropping a little
error per pixel; reproduced as-is.

### Opcode coverage and probe results

`cmd.rs` carried 20 opcodes; the Android `BasePrint.java` declares 54, and
`AdjustManager` a further 5 in what looked like empty space. An earlier reading
of the Electron editor alone concluded the vocabulary was complete — it is not,
and that conclusion was wrong. All are now present in `cmd.rs`.

**Almost none have a call site.** The app declares and never sends
`PAPER_BACK`, `HTIME_RD`/`HTIME_SET`, every `YINWEI` setter, `SET_OPTLEVEL`,
`MAT_AUTHEN_RESULT`, `READ_RANDOM`/`VERIFY_RANDOM`, `SET_TIMESTAMP`,
`FORCEUPDATE`, `SET_DENSITY`, `SET_HEADRATE`. Exercised ones are
`0x39 SET_TB_YINWEI` (G-series, `sendCmd(cmd, value, [0u8; 64])`),
`0xBC CHECK_OPTLEVEL` (**`T50PlusPrint`**, `sendCmdStartTrans(cmd, i, i2, [0u8; 64])`),
`0x18 STRD_MAT`, and `0x67`/`0x68 RD`/`WR_DEV_OPT`.

**Unallocated numbers: 171** inside `0x10`–`0xF0`, the largest holes being
`0x6B-0xAF` (69), `0xDA-0xEF` (22), `0x45-0x57` (19). AdjustManager filling
`0x60-0x62`/`0x6F`/`0xB2` is the standing warning that these are *unknown*, not
unused.

**Probing.** USB HID replies do not echo the command byte
(`usb_transport::validate_response` — any non-empty reply is treated as an ack),
so presence of a reply proves nothing. The discriminator is reply *content*
against a baseline captured from deliberately unallocated opcodes. Not-implemented
answers `08 00 00 10 00 00 00 b0 04 …`.

`supvan-cli probe-reads` does this, with a deny-list refusing anything that
writes, moves paper, or falls in `0xC0-0xEF` — an unrecognised opcode there
risks leaving the unit in a bootloader awaiting an image.

Result on a T50M Pro (`T0117A2410211517`), 15 read-only opcodes:

| opcode | result |
|--------|--------|
| `0xB1 RD_TIMESTAMP` | **implemented** — ASCII `"20230412"`, length-prefixed `0x0a` |
| the other 14 | identical to the unallocated baseline — not implemented |

Notably `0x2B HTIME_RD` is **not** implemented, so `0x2C HTIME_SET` almost
certainly is not either: setting heat time directly, bypassing the RFID record,
is not available on this unit.

**`0xBA PAPER_BACK` is not implemented either.** Tested at params 0, 8 and 100
via `supvan-cli paper-back` (its own command, since `probe-reads` guarantees it
cannot move paper): every reply was byte-identical to an unallocated opcode, the
mechanism never moved, and status was unchanged. So while the opcode *exists* in
the vendor's vocabulary — correcting the earlier claim that no backfeed opcode
does — this firmware does not honour it. Combined with the inert `savepaper`
bit, there is no way to reprint over a label that has already been fed.

**Status flag — `FirmwareNeedUpgrade`.** The Linux tool decodes a "firmware
needs upgrade" flag from status byte `[3] & 0x20` (G-series `gPrintFlag.js`) —
the printer itself signals stale firmware, a natural trigger for a future
updater. The exact byte offset in our BT/USB status frames is unverified, so
`status.rs` does not decode it yet.

### Print job flow (per transport)

Supvan ships **one tool per transport** and the two disagree on several
job-level details. Mirror the tool that actually drives the transport —
`transport::Flow` (`Android` for SPP/BLE, `Electron` for USB HID) carries the
differences. Sources: `T50PlusPrint.{doPrint,transfer,multiCompression}`
smali (Android) and `t5080PrintUtils.js` / `t5080imageEncodeUtils.js` /
`BaseSupVanPrint.js` from the KatasymbolEditor 1.1.1 source map (Electron).

Shared skeleton:

1. `CHECK_DEVICE`, then poll `INQUIRY_STA` until `deviceBusy` clears.
2. `START_PRINT`, then poll until `printingStation` is set.
3. Per block: poll `INQUIRY_STA` until `BufSta` (MSTA bit 0) is **0**, then
   `NEXT_ZIPPEDBULK` → data → `BUF_FULL(len, speed)`. No feed-time waits —
   both tools send the next block as soon as the firmware has room.
4. Poll until `printingStation` clears. Neither tool sends `STOP_PRINT` on a
   normal finish.

| | Android (SPP / BLE) | Electron (USB HID) |
|---|---|---|
| `START_PRINT` param | 0 | 1 |
| `NEXT_ZIPPEDBULK` params | (512, packet count) | compressed length |
| data framing | 506-byte `AA BB` packets (500 B payload) in 512-byte `7E 5A FC 01 10 02` frames, written 4 × 128 B with 10 ms gaps; each packet acked | raw bytes in 64-byte reports, zero-padded, no header; **one** reply after the last report |
| gap before `BUF_FULL` | 20 ms | bulk reply + 150 ms (two timers) |
| gap after `BUF_FULL` reply | none | 100 ms |
| `BufSta` poll | every 20 ms | every 5 ms, ~200 tries |
| block packing | ≤ 4 buffers per block, shrink until ≤ 4096 B | whole page in one block if the buffers, compressed singly, sum ≤ 4000 B; otherwise **one buffer per block** |
| speed (`BUF_FULL` param 2) | running minimum of `calc_speed(block_len / buffers)`; app overrides: 15 inverse labels, 5 photos, 20 rotated barcodes. We apply the page minimum to every block (a mid-page speed change leaves a seam) and cap it by link throughput (below) | 60 single-block page, 20 split page |

**Bluetooth throughput is the printhead's limit.** Every 512-byte packet waits
for its ack, so a dense halftone buffer — ~2.5 KB for 10.5 mm — takes
0.4–0.8 s to deliver: the per-packet ack latency swings between ~20 and ~90 ms
with the radio link, job to job, for identical traffic. The speed value tracks
feed rate in roughly mm/s, and `calc_speed` picks 25 for that density, so the
head outruns the link and pauses mid-label, leaving stripes. The vendor app
only avoids it because its editor forces speed 5 for photos. Two fixes, both
verified on a dense full-bleed 80 mm Floyd–Steinberg ramp:

- read each reply only until its `7E 5A` length field says it is complete,
  instead of idling a fixed 50 ms for stragglers;
- measure the link instead of assuming it. The speed rides on BUF_FULL, which
  follows the data, so `Printer` times the first block's NEXT_ZIPPEDBULK
  round trip and per-byte transfer before committing, and caps the page at
  the fastest vendor speed that lands each block while the head prints the
  previous one (15 % headroom). Measured: Bluetooth in its slow state → 10;
  USB keeps 20. Any transport, no per-link constants.

Do **not** wait for the last data packet's ack over SPP, although the Android
app does: the firmware sends it only once it has digested the whole block,
~0.4 s on a dense one, which halves throughput.

`T80ProPrint` skips the 10 ms chunk gaps whenever a forced speed is set, so
the firmware tolerates gap-free packets; dropping them is the next lever if
dense pages ever need to go faster.

`compress::Packing` implements both packing rules; `Printer::print_buffers`
picks the one for the connected transport.

### LZMA stream format

LZMA1 "alone": 13-byte header (`5D`, dict 8192 LE, **exact** uncompressed
size as u64 LE), then the payload with **no end-of-payload marker**. Each
block decompresses to `n × 4096` bytes of print buffers.

The missing marker is load-bearing. Android's SDK encoder runs with
`EndMarker = false`; Supvan patched their lzma-js copy to force
`_writeEndMark = false`. liblzma's `.lzma` encoder always writes one, and
with it the firmware loses the tail of every block: the last buffer of a
block went missing, and a page split over several blocks stopped at the first
block boundary (42 mm on an 80 mm label with 4-buffer blocks, a few mm with
1-buffer blocks) while single-block pages printed fine. `compress_lzma` uses
liblzma's raw `LZMA_FILTER_LZMA1EXT` encoder, which omits the marker, and
writes the header itself. Dictionary size in the header is irrelevant to the
firmware (Electron's mode-9 streams declare 32 MiB).

### USB HID: hidraw needs an explicit report ID

The device uses no numbered reports, yet a hidraw write must still start with
the report-ID byte: `usbhid_output_report` strips a leading `0x00` as the ID
and sends the rest. Writing the bare 64 bytes therefore sent any report whose
payload began with `0x00` one byte short, corrupting the compressed stream at
random. `HidrawDevice::write_report` always prefixes `0x00` and writes 65
bytes. Stale input reports are drained before every command so a late reply
can never be read as the answer to the next one (Android's `clear()` does the
same over SPP).

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

Both transports carry the remaining-label count at payload offset 21..25
(LE u32), and both parsers read it — verified on a T50M Pro, which reports
the same count over USB and Bluetooth.

## Known gaps and open questions

| Gap | Where | Impact |
|---|---|---|
| `RD_DEV_NAME` over USB | `usb_transport::parse_device_name_response` | Can't get the printer name from a status query; have to issue `RETURN_MAT` instead. |
| `READ_REV` over USB | `parse_version_response` stub | We can't read the protocol version string over USB. Probably fine; same firmware on both ends. |
| `READ_FWVER` over USB | `parse_firmware_version_response` stub | No way to read firmware version over USB without bigger HID report. |
| BT `device_sn` BCD vs USB ASCII | `status::parse_material` | The two transports report the same physical value but in different encodings; downstream code can't naïvely string-compare. |
| Genuine-tag signature | `supvan-app::record_is_ours` | We assume a real consumable carries a non-zero `MaterialInfo::code` and records we write never do. Unconfirmed — needs a genuine tag's pages 4+, which are password-protected. The one place a wrong guess could overwrite a real roll. |
| Consumable tag password | `generalPassword`, commented out upstream | The `SHA-256(UID ‖ Pwkey)` derivation is plausible but never exercised by the vendor's own host code, so unverified. Blocks reading a genuine tag's record. |

## Appendix: BLE GATT transport (implemented behind the `ble` feature, unverified)

We reach the printers over three transports, all sharing the same 16-byte
command framing and 512-byte data frames:

- **Classic SPP** (RFCOMM): UUID `00001101-0000-1000-8000-00805F9B34FB`,
  channel auto-detected; 512-byte write chunks, ~10 ms inter-chunk drain.
- **USB HID**: `0xC0/0x40` framing, 64-byte reports.
- **BLE GATT**: for BLE-only hardware (E11/E12-class — the vendor gates BLE by
  `printingProcess == 4`). Implemented in `crates/supvan-proto/src/ble.rs` as a
  `BlePipe` driven by the shared [`spp_pipe::SppCodec`]; built only with
  `--features ble` (pulls `bluer` + BlueZ). **Unverified against hardware** — we
  own no BLE printer, so an E11/E12 reporter must validate the live round-trip.

BLE wire details (from the vendor `BLEUtils.java`, as implemented):

- Connect TRANSPORT_LE; BlueZ negotiates the ATT MTU (the app requests 200).
- Auto-detect one of three service/characteristic patterns, first match wins:
  `0000fee7-…` (notify == write `0000FEC1-…`), service
  `0000e0ff-3c17-d293-8e48-14fe2e4da212` (notify `0000ffe1-…`, write
  `0000ffe9-…`), or `0000ff00-…` (notify `0000ff01-…`, write `0000ff02-…`).
- No bonding: the app calls `connectGatt(…, TRANSPORT_LE, PHY_LE_1M)` and never
  `createBond()`. Pairing is not a prerequisite for GATT here.
- Every frame is an ATT **write request** (`BLEUtils.write`), in **128-byte**
  fragments with ~10 ms between them: `BasePrint.transferSplitData` splits each
  512-byte data frame into 4 × 128. `writeNoResponse` exists but the T50/G15
  print flows don't use it for frame transfer.
- Commands/status: wait for a notification echoing the command byte at
  **offset 7**, polled up to ~4 s (the app loops 200 × 20 ms).
- Bulk image data is **not acked** over BLE — `transferSplitData` only reads a
  reply in its `CLASSIC_BLUETOOTH` branch. `SppPipe::acks_data_frames` carries
  this difference into the shared codec.
- Discovery (`crates/supvan-app/src/ble_discover.rs`): LE-transport scan, keep
  advertisers matching name `^[TGD]\d{2}`, MAC OUI `A4:93:40`, and one of the
  three GATT services above. Properties are read after the scan window closes,
  because BlueZ fills `Name`/`UUIDs` in only after `DeviceAdded`.
- `supvan-cli` reaches BLE through a `ble://<address>` target, built with
  `--features ble`.

Known unknowns to confirm on real hardware: whether an E11/E12 will drive
correctly from the **T50 print flow** at all — the vendor runs `printingProcess
== 4` through `G15Print`, not `T50PlusPrint`.

## See also

- `docs/HARDWARE.md` — board survey: the Bluetooth SoC, motor driver and the
  5-pad debug port, plus why firmware is the only route to the two unverified
  items above.
- `crates/supvan-proto/src/cmd.rs` — command constants + frame builders.
- `crates/supvan-proto/src/status.rs` — BT response parsers + bit
  assignments.
- `crates/supvan-proto/src/spp_pipe.rs` — the `SppPipe` byte-pipe trait and the
  shared `SppCodec` that drives Classic-BT (`rfcomm.rs`) and BLE (`ble.rs`).
- `crates/supvan-proto/src/usb_transport.rs` — the USB HID `Transport` impl;
  the place where per-transport quirks land.
- `crates/supvan-proto/src/printer.rs` — high-level Printer interface;
  one method per command code, returning the parsed shape.
- `crates/supvan-proto/src/printer.rs::print_compressed` — the real-world
  ordering of these commands for a one-page label print.
