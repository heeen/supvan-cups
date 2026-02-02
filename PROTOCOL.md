# Katasymbol / Supvan T50 Pro Bluetooth Protocol

Reverse-engineered from the Android APK (Katasymbol v1.4.20, decompiled with jadx 1.5.1).

Source files referenced:
- `BasePrint.java` - command framing, transport, status parsing
- `T50PlusPrint.java` - print flow, buffer construction, material parsing
- `BluetoothUtils.java` - Classic BT SPP transport
- `BLEUtils.java` - BLE GATT transport
- `PRINTER_FLAG.java` / `MSTA_REG_BITS.java` / `FSTA_REG_BITS.java` - status bits
- `PAGE_REG_BITS.java` - print buffer page flags
- `LzmaUtils.java` - compression parameters

## Transport

### Classic Bluetooth SPP (RFCOMM)

- UUID: `00001101-0000-1000-8000-00805F9B34FB`
- Channel: 1
- Write chunked: 512-byte max chunks, drain RX + 10ms delay between chunks
- Read: poll up to 2000ms in 20ms intervals

### BLE GATT

Three service UUID patterns are supported, auto-detected at connection:

| Service UUID | Notify UUID | Write UUID |
|---|---|---|
| `0000fee7-0000-1000-8000-0805f9b34fb` | `0000FEC1-...` | `0000FEC1-...` (same) |
| `0000e0ff-3c17-d293-8e48-14fe2e4da212` | `0000ffe1-...` | `0000ffe9-...` |
| `0000ff00-0000-1000-8000-00805f9b34fb` | `0000ff01-...` | `0000ff02-...` |

- MTU: 200 (requested via `requestMtu(200)`)
- Notification descriptor: `00002902-0000-1000-8000-00805f9b34fb`
- BLE response: poll `getCurrentValue()` up to 200 x 20ms (4 seconds)

## Command Frame Format

All commands use a 16-byte frame with `0x7E 0x5A` header.

```
Offset  Size  Description
------  ----  -----------
 0      1     Magic 1: 0x7E
 1      1     Magic 2: 0x5A
 2      2     Payload length (little-endian) = frame_size - 4 = 0x000C (12)
 4      1     Protocol ID: 0x10
 5      1     Protocol version: 0x01
 6      1     Marker: 0xAA
 7      1     Command byte
 8      2     Checksum (little-endian): sum of bytes [10..15]
10      1     0x00
11      1     0x01
12      2     Parameter (little-endian) -- or block_size for transfer commands
14      2     0x0000                    -- or block_count for transfer commands
```

### Checksum

```
checksum = sum(frame[10:16]) & 0xFFFF
frame[8] = checksum & 0xFF
frame[9] = (checksum >> 8) & 0xFF
```

### Response Frame

Responses use the same `0x7E 0x5A` header. The command byte at offset 7 echoes
the request command. Status/data bytes follow at offset 14+.

```
Offset  Size  Description
------  ----  -----------
 0      1     0x7E
 1      1     0x5A
 2      2     Payload length (little-endian)
 4      1     Protocol ID: 0x10
 5      1     0x03 (response marker)
 6      1     0x55
 7      1     Command echo
 8      2     Checksum
10-13         (varies)
14+           Response data (command-dependent)
```

## Data Transfer Frame

Used to wrap 506-byte data packets during bulk transfer.

```
Offset  Size  Description
------  ----  -----------
 0      1     0x7E
 1      1     0x5A
 2      2     Payload length: 0x01FC (508)
 4      1     Protocol ID: 0x10
 5      1     Data type: 0x02
 6      506   Data packet (AA BB format, see below)
```

Total frame size: 512 bytes. Sent as 4 x 128-byte chunks over classic BT,
with 10ms delay and RX drain before each chunk.

### Data Packet (AA BB format)

Carried inside the data transfer frame at offset 6.

```
Offset  Size  Description
------  ----  -----------
 0      1     0xAA
 1      1     0xBB
 2      2     Checksum (little-endian): sum of bytes [4..505]
 4      1     Packet index (0-based)
 5      1     Total packet count
 6      500   Payload (LZMA-compressed print buffer chunk)
```

## Command Table

| Byte | Hex | Name | Description |
|------|-----|------|-------------|
| 16 | `0x10` | CMD_BUF_FULL | Signal buffer complete (param: compressed_len, block_count: speed) |
| 17 | `0x11` | CMD_INQUIRY_STA | Query printer status |
| 18 | `0x12` | CMD_CHECK_DEVICE | Check device presence |
| 19 | `0x13` | CMD_START_PRINT | Start print job |
| 20 | `0x14` | CMD_STOP_PRINT | Stop/cancel print job |
| 22 | `0x16` | CMD_RD_DEV_NAME | Read device name (ASCII at offset 22) |
| 23 | `0x17` | CMD_READ_REV | Read protocol version (ASCII at offset 22, e.g. "1.9") |
| 24 | `0x18` | CMD_STRD_MAT | Store material data |
| 25 | `0x19` | CMD_STRD_MAT_INFO | Store material info |
| 34 | `0x22` | CMD_READ_DPI | Read DPI value |
| 36 | `0x24` | CMD_RD_LAB_YINWEI | Read label offset |
| 37 | `0x25` | CMD_SET_LAB_YINWEI | Set label offset |
| 38 | `0x26` | CMD_RD_HD_YINWEI | Read head offset |
| 39 | `0x27` | CMD_SET_HD_YINWEI | Set head offset |
| 46 | `0x2E` | CMD_PAPER_SKIP | Feed to next label |
| 48 | `0x30` | CMD_RETURN_MAT | Read material/consumable info |
| 49 | `0x31` | CMD_RD_DEV_DPI | Read device DPI (alias of CMD_RD_COnLAB_YINWEI) |
| 51 | `0x33` | CMD_SET_PRTMODE | Set print mode |
| 53 | `0x35` | CMD_SEND_INF | Send info |
| 55 | `0x37` | CMD_SET_BLTCONTROL | Set Bluetooth control |
| 56 | `0x38` | CMD_SET_RL_YINWEI | Set right/left offset |
| 57 | `0x39` | CMD_SET_TB_YINWEI | Set top/bottom offset |
| 65 | `0x41` | CMD_READ_POWER_OFF_TIME | Read auto power-off timeout |
| 66 | `0x42` | CMD_SET_POWER_OFF_TIME | Set auto power-off timeout |
| 67 | `0x43` | CMD_READ_BUZZER_KEY | Read buzzer/key config |
| 68 | `0x44` | CMD_SET_BUZZER_KEY | Set buzzer/key config |
| 88 | `0x58` | CMD_RD_USER_INF | Read user info |
| 92 | `0x5C` | CMD_NEXT_ZIPPEDBULK | Start LZMA compressed bulk transfer |
| 93 | `0x5D` | CMD_SET_RFID_DATA | Set RFID consumable data |
| 96 | `0x60` | CMD_ADJ_BCUT | Adjust blade cutter |
| 103 | `0x67` | CMD_RD_DEV_OPT | Read device options |
| 104 | `0x68` | CMD_WR_DEV_OPT | Write device options |
| 176 | `0xB0` | CMD_SET_TIMESTAMP | Set timestamp |
| 177 | `0xB1` | CMD_RD_TIMESTAMP | Read timestamp |
| 178 | `0xB2` | CMD_RD_CONTINUE | Read continuation (multi-part responses) |
| 182 | `0xB6` | CMD_MAT_AUTHEN_RESULT | Material authentication result |
| 186 | `0xBA` | CMD_PAPER_BACK | Reverse feed |
| 188 | `0xBC` | CMD_CHECK_OPTLEVEL | Check option level |
| 189 | `0xBD` | CMD_READ_OPTLEVEL | Read option level |
| 190 | `0xBE` | CMD_SET_OPTLEVEL | Set option level |
| 197 | `0xC5` | CMD_READ_FWVER | Read firmware version (byte at offset 22) |
| 198 | `0xC6` | CMD_START_FWUPDATA | Start firmware update |
| 201 | `0xC9` | CMD_BLTCMD_SET_HEADRATE | Set print head rate |
| 211 | `0xD3` | CMD_FORCEUPDATE | Force firmware update |
| 213 | `0xD5` | CMD_READ_RANDOM | Read random (authentication) |
| 214 | `0xD6` | CMD_VERIFY_RANDOM | Verify random (authentication) |
| 217 | `0xD9` | CMD_BLTCMD_SET_DENSITY | Set print density |

## Status Response (CMD_INQUIRY_STA = 0x11)

Response bytes at offsets 14-19 map to printer status flags:

### Byte 14 (MSTA low)

| Bit | Mask | Name | Description |
|-----|------|------|-------------|
| 0 | `0x01` | BufSta | Buffer full (1 = full, wait before sending more) |
| 1 | `0x02` | LabRwErr | Label read/write error |
| 2 | `0x04` | LabEnd | Label roll end |
| 3 | `0x08` | LabXhErr | Label mode mismatch error |
| 4 | `0x10` | RibRwErr | Ribbon read/write error |
| 5 | `0x20` | RibEnd | Ribbon end |
| 6 | `0x40` | LowBattery / RibXhErr | Low battery or ribbon mismatch |

### Byte 15 (MSTA high)

| Bit | Mask | Name | Description |
|-----|------|------|-------------|
| 0-1 | `0x03` | SysErr | System error code |
| 2 | `0x04` | ComExeSta / DeviceBusy | Device is busy processing |
| 3 | `0x08` | CutNeedClr / HeadTempHigh | Printhead temperature too high |

### Byte 16 (FSTA low)

| Bit | Mask | Name | Description |
|-----|------|------|-------------|
| 0 | `0x01` | fB1Sta | Buffer 1 status |
| 1 | `0x02` | fB2Sta | Buffer 2 status |
| 2 | `0x04` | fB3Sta | Buffer 3 status |
| 3 | `0x08` | CoverOpen | Cover is open |
| 4 | `0x10` | InsertUSB | USB cable connected |
| 6 | `0x40` | PrintingStation | Printer is actively printing |
| 7 | `0x80` | sDevBusy | Sub-device busy |

### Byte 17 (FSTA high)

| Bit | Mask | Name | Description |
|-----|------|------|-------------|
| 0 | `0x01` | LabelNotInstalled | No label roll loaded |
| 7 | `0x80` | Charging | Battery is charging |

### Bytes 18-19

Little-endian 16-bit print count (number of labels printed in current job).

## Material / Consumable Info (CMD_RETURN_MAT = 0x30)

Response parsing (offsets into the response frame):

| Offset | Size | Description |
|--------|------|-------------|
| 22-28 | 7 | UUID (hex) |
| 29-36 | 8 | Code (hex) |
| 37-38 | 2 | Serial number (big-endian: byte[38]<<8 | byte[37]) |
| 39 | 1 | Label type |
| 40 | 1 | Label width (mm) |
| 41 | 1 | Label height (mm) |
| 42 | 1 | Gap (mm) |
| 43-46 | 4 | Remaining label count (little-endian 32-bit) |
| 51-56 | 6 | Device serial number (BCD-encoded, 2 digits per byte) |

## Print Buffer Format

Each print buffer is 4096 bytes, containing a 14-byte header followed by
column-major 1-bit bitmap data.

```
Offset  Size  Description
------  ----  -----------
 0      2     Checksum (little-endian)
 2      2     PAGE_REG_BITS (page flags, see below)
 4      2     Column count in this buffer (little-endian)
 6      1     Bytes per line (width_dots / 8)
 7      1     Reserved (0)
 8      2     Margin top in dots (little-endian, range 1-900)
10      2     Margin bottom in dots (little-endian, range 1-900)
12      1     Density / red deepness (0-15)
13      1     Reserved (0)
14      4082  Bitmap data (column-major, LSB-first, 1 bit per pixel)
```

### Buffer Checksum

```
checksum = sum(buffer[2:14])
for each 256-byte boundary i (1, 2, 3, ...):
    checksum += buffer[(i * 256) - 1]
```

### PAGE_REG_BITS (bytes 2-3)

#### Byte 0

| Bits | Name | Description |
|------|------|-------------|
| 1 | PageSt | First buffer of the page |
| 2 | PageEnd | Last buffer of the page |
| 3 | PrtEnd | End of entire print job |
| 4-6 | Cut | Cut mode (3 bits) |
| 7 | Savepaper | Save paper mode |

#### Byte 1

| Bits | Name | Description |
|------|------|-------------|
| 0-1 | FirstCut | First cut mode |
| 2-5 | Nodu | Density (0-15) |
| 6-7 | Mat | Material type (shifted left by 6) |

### Image Encoding

- The source bitmap is rotated -90 degrees before encoding
- After rotation: stored as column-major, LSB-first, 1-bit packed
- Each column is `bytes_per_line` bytes wide (= `width_dots / 8`)
- T50 Pro: 8 dots/mm (203 DPI), max 48mm width = 384 dots max

### Buffer Splitting

A single label image is split across multiple 4096-byte buffers:
- `max_cols_per_buffer = (4096 - 14) / bytes_per_line`
- First buffer: `PageSt=1`
- Last buffer: `PageEnd=1, PrtEnd=1`
- Middle buffers: all flags 0

## LZMA Compression

Each 4096-byte print buffer is LZMA-compressed before transfer. The printer
firmware has limited RAM -- parameters must match exactly:

| Parameter | Value | Notes |
|-----------|-------|-------|
| Format | LZMA1 (alone) | Not LZMA2 |
| Dictionary size | 8192 | 8 KB -- critical, larger values fail |
| lc | 3 | Literal context bits |
| lp | 0 | Literal position bits |
| pb | 2 | Position bits |
| Algorithm | 2 / NORMAL | Max compression |
| Match finder | BT4 | Binary tree, 4 bytes |
| Nice length | 128 | Fast bytes |
| End marker | false | No end-of-stream marker |

Source: `LzmaUtils.java`

## Print Flow

From `T50PlusPrint.doPrint()`:

```
1. CHECK_DEVICE (0x12)
   - Verify printer is present

2. Poll INQUIRY_STA (0x11) until device_busy == 0
   - If already printing, send STOP_PRINT
   - Check for errors: label_rw_error, label_mode_error, cover_open, etc.

3. START_PRINT (0x13)
   - Puts printer into print mode

4. Poll INQUIRY_STA until printing == 1 (PrintingStation active)

5. For each compressed buffer:
   a. Poll INQUIRY_STA until buf_full == 0 (every 20ms)
      - Do NOT check printing flag between buffers
      - Send next buffer as fast as possible to avoid printer timeout
   b. CMD_NEXT_ZIPPEDBULK (0x5C) with block_size=512, block_count=num_packets
   c. Send data packets (AA BB) wrapped in data frames (7E 5A)
      - Each 512-byte frame split into 4 x 128 chunks
      - 10ms delay between chunks
   d. 20ms delay after last data packet
   e. CMD_BUF_FULL (0x10) with param=compressed_length, block_count=speed

6. Poll INQUIRY_STA until printing == 0 and device_busy == 0
   - Print job complete
```

### Timing

- Inter-chunk delay: 10ms (within each 512-byte frame)
- Buffer poll interval: 20ms
- No extra delay between buffers (critical -- printer exits print mode quickly)
- Status poll timeout: 100ms between attempts during wait phases

## Device Info (T50 Pro / M50 Pro)

Discovered from a Katasymbol M50 Pro unit:

| Property | Value |
|----------|-------|
| BT name | T0117A2410211517 |
| BT address | A4:93:40:A0:87:57 |
| Device name | T50Pro |
| Firmware version | 1 |
| Protocol version | 1.9 |
| DPI | 8 dots/mm (203 DPI) |
| Max width | 48 mm (384 dots) |
| Label tested | 40mm x 30mm, type 1, gap 3 |

## USB HID Protocol

The Windows Electron app uses a simpler framing over USB HID. The command set,
print flow, buffer format, and LZMA parameters are identical to Bluetooth --
only the transport framing differs.

Source: Electron app bundle (webpack, minified JS), extracted with babel/AST.

### USB HID Transport

- WebHID API: `navigator.hid`
- Reports: 64-byte HID output reports via `sendReport(0, data)`
- Responses: 8 bytes via `oninputreport` callback
- No checksums, no protocol ID, no framing magic beyond the 0xC0/0x40 header

### USB HID Command Frame (SendCmd)

8 bytes. Used for most commands.

```
Offset  Size  Description
------  ----  -----------
 0      1     Magic 1: 0xC0
 1      1     Magic 2: 0x40
 2      1     Parameter high byte (BIG-endian, opposite of Bluetooth!)
 3      1     Parameter low byte
 4      1     Command byte (same command set as Bluetooth)
 5      1     0x00
 6      1     Length: 0x08
 7      1     0x00
```

### USB HID Extended Command Frame (SendCmdTwo)

10 bytes. Used for commands with two parameters (e.g. CMD_BUF_FULL where
param1 = compressed length, param2 = speed).

```
Offset  Size  Description
------  ----  -----------
 0-7          Same as SendCmd
 8      1     Second parameter high byte (big-endian)
 9      1     Second parameter low byte
```

Note: SendCmdTwo is sent with a 50ms delay before write.

### USB HID Data Transfer

Bulk data (LZMA-compressed print buffers) is split into 64-byte HID reports
and sent sequentially via `sendReport(0, chunk)`. There is no 0xAA/0xBB
data packet wrapping or 0x7E/0x5A frame wrapping -- the raw compressed data
is chunked directly into HID reports.

```
for each 64-byte chunk of data:
    pad to 64 bytes with 0x00 if needed
    hidDevice.sendReport(0, chunk)
```

### USB HID Response

Responses arrive as HID input reports (8 bytes). The `oninputreport` handler
reads `event.data.buffer` and extracts bytes `[0..7]`.

Status parsing uses bytes `[1..8]` of the raw report (offset by 1 from the
report buffer start):

```
status[0] = response[1]    MSTA low   (same bit layout as BT byte 14)
status[1] = response[2]    MSTA high  (same bit layout as BT byte 15)
status[2] = response[3]    FSTA low   (same bit layout as BT byte 16)
status[3] = response[4]    FSTA high  (same bit layout as BT byte 17)
status[4] = response[5]    print count low
status[5] = response[6]    print count high
status[6] = response[7]
status[7] = response[8]
```

The status bit definitions (BufSta, PrtSta, CoverOpen, etc.) are identical
to the Bluetooth protocol.

### Bluetooth vs USB HID Comparison

| | Bluetooth (0x7E 0x5A) | USB HID (0xC0 0x40) |
|---|---|---|
| Command size | 16 bytes | 8 or 10 bytes |
| Checksums | Sum of bytes [10..15] | None |
| Protocol ID / version | 0x10, 0x01 in header | None |
| Parameter byte order | Little-endian | Big-endian |
| Data framing | 506B AA/BB packet in 512B frame, split 4x128B | Raw 64-byte HID reports |
| Response size | Variable (20+ bytes) | 8 bytes |
| Status byte offset | Response bytes [14..19] | Response bytes [1..6] |
| Command bytes | 0x10-0xD9 | Same |
| Print flow | CHECK_DEVICE -> STATUS -> START -> transfer -> BUF_FULL | Same |
| Print buffers | 4096 bytes, 14-byte header | Same |
| LZMA compression | dict=8192, lc=3, lp=0, pb=2 | Same |
