#!/usr/bin/env python3
"""
Katasymbol M50 Pro test print script over Bluetooth RFCOMM.

Protocol reverse-engineered from the Android APK (Katasymbol v1.4.20, decompiled with jadx).
Key source files:
  - BasePrint.java:      sendCmd(), sendCmdStartTrans(), transferSplitData(), classicBluetoothStatus()
  - T50PlusPrint.java:   transfer(), getMaterial(), doPrint()
  - BluetoothUtils.java: Classic BT SPP transport (RFCOMM, 512-byte chunk writes)
  - PRINTER_FLAG.java:   Status bit parsing
  - PAGE_REG_BITS.java:  Print buffer flags

Two command formats exist:
  1. Standard 16-byte (0x7E 0x5A header) - used for most commands
  2. Short 8-byte (0xC0 0x40 header)     - used for some status queries

Data transfer uses 506-byte packets (0xAA 0xBB header) wrapped in
512-byte frames (0x7E 0x5A header), sent as 4x128-byte chunks.

Usage:
  python3 test_print.py                       # auto-connect via BT socket, probe only
  python3 test_print.py --print               # auto-connect + test print
  python3 test_print.py bt:A4:93:40:A0:87:57  # explicit BT address
  python3 test_print.py /dev/rfcomm0           # use serial device instead

Prerequisites:
  - Pair: bluetoothctl pair A4:93:40:A0:87:57
  - No rfcomm bind or sudo needed (uses direct BT socket)
"""

import sys
import os
import time
import struct
import socket
import lzma as lzma_mod

try:
    import serial
except ImportError:
    serial = None  # Optional: only needed for /dev/rfcomm* mode

BT_ADDRESS = "A4:93:40:A0:87:57"
BT_CHANNEL = 1

# ============================================================
# Protocol constants (from BasePrint.java)
# Apache POI Ptg constants resolved to their actual values:
#   UnionPtg.sid=0x10, RangePtg.sid=0x11, UnaryPlusPtg.sid=0x12,
#   UnaryMinusPtg.sid=0x13, PercentPtg.sid=0x14, Ptg.CLASS_ARRAY=0x40
# ============================================================

CMD_BUF_FULL        = 0x10   # Send print data / buffer status
CMD_INQUIRY_STA     = 0x11   # Query printer status
CMD_CHECK_DEVICE    = 0x12   # Check device presence
CMD_START_PRINT     = 0x13   # Start print job
CMD_STOP_PRINT      = 0x14   # Stop print job
CMD_RD_DEV_NAME     = 0x16   # Read device name
CMD_READ_REV        = 0x17   # Read revision
CMD_PAPER_SKIP      = 0x2E   # Paper skip/feed
CMD_RETURN_MAT      = 0x30   # Return material (label) info
CMD_NEXT_ZIPPEDBULK = 0x5C   # Start LZMA compressed bulk transfer
CMD_SET_RFID_DATA   = 0x5D   # Set RFID data
CMD_READ_FWVER      = 0xC5   # Read firmware version

# Protocol magic bytes
MAGIC1    = 0x7E
MAGIC2    = 0x5A
PROTO_ID  = 0x10
PROTO_VER = 0x01
MARKER_AA = 0xAA
DATA_TYPE = 0x02

# Data packet magic
DATA_MAGIC1 = 0xAA
DATA_MAGIC2 = 0xBB

# T50 Plus series: 8 dots/mm DPI, max 48mm printhead width -> 384 dots
# The Android app forces mMatWidth=48 for T50Plus series (line 140),
# regardless of the actual label width.  The image canvas is 48mm wide
# with the label content centered.
DPI = 8
MAX_WIDTH_MM = 48
PRINTHEAD_WIDTH_MM = 48  # Always 48 for T50Plus (printhead width, not label width)
MAX_BUF_DATA = 4074  # R2.drawable.sf5334_ = 4074 (max image data bytes per buffer)


# ============================================================
# Command builders (from BasePrint.sendCmd / sendCmdStartTrans)
# ============================================================

def make_cmd(cmd, param=0):
    """Build a standard 16-byte command (0x7E 0x5A format).

    Byte layout (from BasePrint.sendCmd):
      [0]  0x7E   Magic 1
      [1]  0x5A   Magic 2
      [2]  0x0C   Length low (12 = packet_size - 4)
      [3]  0x00   Length high
      [4]  0x10   Protocol ID
      [5]  0x01   Protocol version
      [6]  0xAA   Marker
      [7]  CMD    Command byte
      [8]  CHK_L  Checksum low  (sum of bytes[10..15])
      [9]  CHK_H  Checksum high
      [10] 0x00
      [11] 0x01
      [12] P_LO   Parameter low
      [13] P_HI   Parameter high
      [14] 0x00
      [15] 0x00
    """
    pkt = bytearray(16)
    pkt[0]  = MAGIC1
    pkt[1]  = MAGIC2
    pkt[2]  = 0x0C
    pkt[3]  = 0x00
    pkt[4]  = PROTO_ID
    pkt[5]  = PROTO_VER
    pkt[6]  = MARKER_AA
    pkt[7]  = cmd & 0xFF
    pkt[10] = 0x00
    pkt[11] = 0x01
    pkt[12] = param & 0xFF
    pkt[13] = (param >> 8) & 0xFF
    chk = sum(pkt[10:16])
    pkt[8] = chk & 0xFF
    pkt[9] = (chk >> 8) & 0xFF
    return bytes(pkt)


def make_cmd_start_trans(cmd, block_size, block_count):
    """Build a 16-byte start-transfer command.

    Same structure as make_cmd but bytes 12-15 carry block_size and block_count.
    Used by BasePrint.sendCmdStartTrans() and for CMD_BUF_FULL (where
    block_size=compressed_length, block_count=speed).
    """
    pkt = bytearray(16)
    pkt[0]  = MAGIC1
    pkt[1]  = MAGIC2
    pkt[2]  = 0x0C
    pkt[3]  = 0x00
    pkt[4]  = PROTO_ID
    pkt[5]  = PROTO_VER
    pkt[6]  = MARKER_AA
    pkt[7]  = cmd & 0xFF
    pkt[10] = 0x00
    pkt[11] = 0x01
    pkt[12] = block_size & 0xFF
    pkt[13] = (block_size >> 8) & 0xFF
    pkt[14] = block_count & 0xFF
    pkt[15] = (block_count >> 8) & 0xFF
    chk = sum(pkt[10:16])
    pkt[8] = chk & 0xFF
    pkt[9] = (chk >> 8) & 0xFF
    return bytes(pkt)


def make_data_packet(data_chunk, pkt_idx, pkt_total):
    """Build a 506-byte data packet (0xAA 0xBB format).

    From T50PlusPrint.transfer():
      [0]     0xAA
      [1]     0xBB
      [2]     CHK_LO   sum of bytes[4..505]
      [3]     CHK_HI
      [4]     PKT_IDX  0-based packet index
      [5]     PKT_TOT  total packet count
      [6-505] DATA     500 bytes of payload
    """
    pkt = bytearray(506)
    pkt[0] = DATA_MAGIC1
    pkt[1] = DATA_MAGIC2
    pkt[4] = pkt_idx & 0xFF
    pkt[5] = pkt_total & 0xFF
    chunk = data_chunk[:500]
    pkt[6:6 + len(chunk)] = chunk
    chk = sum(pkt[4:506])
    pkt[2] = chk & 0xFF
    pkt[3] = (chk >> 8) & 0xFF
    return bytes(pkt)


def wrap_data_frame(payload):
    """Wrap a 506-byte data packet in a 512-byte transfer frame.

    From BasePrint.transferSplitData():
      [0]     0x7E
      [1]     0x5A
      [2]     0xFC   length low  (508 = 0x01FC)
      [3]     0x01   length high
      [4]     0x10   protocol ID
      [5]     0x02   data transfer type
      [6-511] 506 bytes of payload (the AA BB packet)
    """
    frame = bytearray(512)
    frame[0] = MAGIC1
    frame[1] = MAGIC2
    frame[2] = 0xFC
    frame[3] = 0x01
    frame[4] = PROTO_ID
    frame[5] = DATA_TYPE
    frame[6:6 + min(len(payload), 506)] = payload[:506]
    return bytes(frame)


# ============================================================
# Response parsing
# ============================================================

def parse_status(data):
    """Parse printer status from CMD_INQUIRY_STA response.

    From BasePrint.refreshStatus() and PRINTER_FLAG.Refresh():
    The response is in 0x7E 0x5A format. Status bytes are at [14..19],
    which map to bArr2[0..5] in refreshStatus():
      bArr2[0] = resp[14]   MSTA low
      bArr2[1] = resp[15]   MSTA high
      bArr2[2] = resp[16]   FSTA low
      bArr2[3] = resp[17]   FSTA high
      bArr2[4] = resp[18]   print count low
      bArr2[5] = resp[19]   print count high
    """
    if not data or len(data) < 20:
        return None
    if data[0] != MAGIC1 or data[1] != MAGIC2:
        return None
    if data[7] != CMD_INQUIRY_STA:
        return None

    b0, b1, b2, b3 = data[14], data[15], data[16], data[17]
    status = {
        # MSTA_REG (bytes 14-15)
        'buf_full':          bool(b0 & 0x01),
        'label_rw_error':    bool(b0 & 0x02),
        'label_end':         bool(b0 & 0x04),
        'label_mode_error':  bool(b0 & 0x08),
        'ribbon_rw_error':   bool(b0 & 0x10),
        'ribbon_end':        bool(b0 & 0x20),
        'low_battery':       bool(b0 & 0x40),
        # MSTA_REG high
        'device_busy':       bool(b1 & 0x04),
        'head_temp_high':    bool(b1 & 0x08),
        # FSTA_REG (bytes 16-17)
        'cover_open':        bool(b2 & 0x08),
        'insert_usb':        bool(b2 & 0x10),
        'printing':          bool(b2 & 0x40),
        # FSTA_REG high
        'label_not_installed': bool(b3 & 0x01),
    }
    if len(data) >= 20:
        status['print_count'] = (data[18] & 0xFF) | ((data[19] & 0xFF) << 8)
    return status


def parse_material(data):
    """Parse material info from CMD_RETURN_MAT response.

    From T50PlusPrint.getMaterial() (JADX partial decompilation):
      UUID   = hex(bytes 22..28)     7 bytes
      Code   = hex(bytes 29..36)     8 bytes
      SN     = int(hex(byte38)+hex(byte37), 16)  big-endian 16-bit
      Type   = byte 39 & 0xFF
      Width  = byte 40              mm
      Height = byte 41              mm
      Gap    = byte 42
      Remind = bytes 43..46         little-endian 32-bit
    """
    if not data or len(data) < 43:
        return None
    if data[0] != MAGIC1 or data[1] != MAGIC2:
        return None
    if data[7] != CMD_RETURN_MAT:
        return None

    mat = {}
    mat['uuid']   = data[22:29].hex().upper()
    mat['code']   = data[29:37].hex().upper()
    sn_hex = f'{data[38]:02x}{data[37]:02x}'
    mat['sn']     = int(sn_hex, 16)
    mat['type']   = data[39] & 0xFF
    mat['width']  = data[40]
    mat['height'] = data[41]
    mat['gap']    = data[42]
    if len(data) >= 47:
        mat['remind'] = (data[43] | (data[44] << 8)
                         | (data[45] << 16) | (data[46] << 24))
    if len(data) >= 57:
        dev_sn = ''.join(f'{data[51+i]:02d}' for i in range(6))
        mat['device_sn'] = dev_sn
    return mat


def fmt_hex(data, maxlen=40):
    """Format bytes as hex string, truncated."""
    if not data:
        return "(none)"
    h = data[:maxlen].hex()
    return h + ("..." if len(data) > maxlen else "")


# ============================================================
# Printer communication class
# ============================================================

class SupvanPrinter:
    """Communicate with a Supvan/Katasymbol printer over Bluetooth SPP (RFCOMM).

    Supports two transport modes:
      - Direct Bluetooth socket (bt:<address>) - no sudo/rfcomm needed
      - Serial device (/dev/rfcomm0) - needs prior rfcomm bind
    """

    def __init__(self, target=None, timeout=2.0):
        self.timeout = timeout
        self.sock = None      # BT socket mode
        self.ser = None       # Serial mode
        self.target = target  # 'bt:XX:XX:XX:XX:XX:XX' or '/dev/rfcomm*'

    def connect(self):
        if self.target and self.target.startswith('bt:'):
            addr = self.target[3:]
            self._connect_bt(addr)
        elif self.target and self.target.startswith('/dev/'):
            self._connect_serial(self.target)
        else:
            # Default: try direct BT socket first, fall back to serial
            try:
                self._connect_bt(BT_ADDRESS)
            except (OSError, ConnectionError) as e:
                print(f"  Direct BT failed ({e}), trying /dev/rfcomm0...")
                if os.path.exists('/dev/rfcomm0'):
                    self._connect_serial('/dev/rfcomm0')
                else:
                    raise

    def _connect_bt(self, addr, channel=BT_CHANNEL):
        """Connect via direct Bluetooth RFCOMM socket (no sudo needed)."""
        print(f"Connecting to {addr} channel {channel} via BT socket...")
        self.sock = socket.socket(
            socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM
        )
        self.sock.settimeout(self.timeout)
        self.sock.connect((addr, channel))
        print(f"Connected to {addr} (direct BT socket)")

    def _connect_serial(self, port):
        """Connect via serial device (/dev/rfcomm*)."""
        if serial is None:
            raise RuntimeError("pyserial required for serial mode: pip install pyserial")
        print(f"Opening {port}...")
        self.ser = serial.Serial(
            port=port, baudrate=115200,
            timeout=self.timeout, write_timeout=self.timeout,
        )
        self.ser.reset_input_buffer()
        print(f"Connected to {port} (serial)")

    def close(self):
        if self.sock:
            try:
                self.sock.close()
            except Exception:
                pass
            self.sock = None
        if self.ser:
            try:
                self.ser.close()
            except Exception:
                pass
            self.ser = None

    def _drain(self):
        """Drain pending input data (like Android app's clear())."""
        if self.sock:
            self.sock.setblocking(False)
            try:
                while True:
                    d = self.sock.recv(1024)
                    if not d:
                        break
            except (BlockingIOError, socket.timeout):
                pass
            finally:
                self.sock.settimeout(self.timeout)
        elif self.ser and self.ser.in_waiting > 0:
            self.ser.read(self.ser.in_waiting)

    def _write_chunked(self, data, chunk_size=512, delay_ms=10):
        """Write data in chunks with delays (matching BluetoothUtils.ConnectedThread.write)."""
        offset = 0
        remaining = len(data)
        while remaining > 0:
            n = min(remaining, chunk_size)
            self._drain()
            if self.sock:
                self.sock.sendall(data[offset:offset + n])
            else:
                self.ser.write(data[offset:offset + n])
                self.ser.flush()
            time.sleep(delay_ms / 1000.0)
            offset += n
            remaining -= n

    def _read_response(self, max_wait_ms=2000, poll_ms=20):
        """Read response by polling (matching BluetoothUtils.ConnectedThread.read)."""
        response = bytearray()
        polls = max_wait_ms // poll_ms
        if self.sock:
            self.sock.settimeout(poll_ms / 1000.0)
            for _ in range(polls):
                try:
                    data = self.sock.recv(512)
                    if data:
                        response.extend(data)
                        # Brief extra wait for trailing bytes
                        self.sock.settimeout(0.05)
                        try:
                            data = self.sock.recv(512)
                            if data:
                                response.extend(data)
                        except socket.timeout:
                            pass
                        break
                except socket.timeout:
                    continue
            self.sock.settimeout(self.timeout)
        else:
            for _ in range(polls):
                time.sleep(poll_ms / 1000.0)
                avail = self.ser.in_waiting
                if avail > 0:
                    response.extend(self.ser.read(avail))
                    time.sleep(0.05)
                    avail = self.ser.in_waiting
                    if avail > 0:
                        response.extend(self.ser.read(avail))
                    break
        return bytes(response) if response else None

    def send_cmd(self, cmd, param=0, retries=1):
        """Send a standard 16-byte command and read response.

        Mirrors BasePrint.classicBluetoothStatus(): write then read.
        On failure, retries once (except for CMD_RETURN_MAT which doesn't retry).
        """
        pkt = make_cmd(cmd, param)
        print(f"  TX: {fmt_hex(pkt)}")
        self._write_chunked(pkt)
        resp = self._read_response()
        if resp:
            print(f"  RX: {fmt_hex(resp)}")
        else:
            print(f"  RX: (no response)")
            if retries > 0 and cmd != CMD_RETURN_MAT:
                print(f"  Retrying...")
                return self.send_cmd(cmd, param, retries - 1)
        return resp

    def send_cmd_start_trans(self, cmd, block_size, block_count, retries=0):
        """Send a start-transfer command and read response."""
        pkt = make_cmd_start_trans(cmd, block_size, block_count)
        print(f"  TX: {fmt_hex(pkt)}")
        self._write_chunked(pkt)
        resp = self._read_response()
        if resp:
            print(f"  RX: {fmt_hex(resp)}")
        else:
            print(f"  RX: (no response)")
        return resp

    def send_data_frame(self, payload_506, read_response=True):
        """Send a 512-byte data frame as 4x128-byte chunks.

        From BasePrint.transferSplitData() with i=1 (split mode):
        - Wraps payload in 512-byte frame with 0x7E 0x5A header
        - Splits into 4 x 128-byte chunks
        - 10ms delay before each chunk, clear() + write + flush
        - Optionally reads response after last chunk
        """
        frame = wrap_data_frame(payload_506)
        # Split into 4 x 128-byte chunks (matching Android transferSplitData)
        for i in range(4):
            chunk = frame[i * 128:(i + 1) * 128]
            time.sleep(0.01)  # 10ms pre-delay (Thread.sleep(i3))
            self._drain()     # clear() before write
            if self.sock:
                self.sock.sendall(chunk)
            else:
                self.ser.write(chunk)
                self.ser.flush()
        if read_response:
            resp = self._read_response()
            if resp:
                print(f"  Data frame RX: {fmt_hex(resp)}")
            else:
                print(f"  Data frame RX: (no response)")
            return resp
        return None

    # ---- High-level commands ----

    def check_device(self):
        print("\n=== CHECK DEVICE (0x12) ===")
        resp = self.send_cmd(CMD_CHECK_DEVICE)
        if resp and len(resp) >= 8 and resp[0] == MAGIC1 and resp[7] == CMD_CHECK_DEVICE:
            print("  -> Device responded OK")
            return True
        return False

    def query_status(self):
        print("\n=== QUERY STATUS (0x11) ===")
        resp = self.send_cmd(CMD_INQUIRY_STA)
        if resp:
            status = parse_status(resp)
            if status:
                print(f"  -> Status: {status}")
                return status
        return None

    def query_material(self):
        print("\n=== QUERY MATERIAL (0x30) ===")
        resp = self.send_cmd(CMD_RETURN_MAT, retries=0)
        if resp:
            mat = parse_material(resp)
            if mat:
                print(f"  -> Material: {mat}")
                return mat
            else:
                print(f"  -> Could not parse material response")
        return None

    def read_device_name(self):
        print("\n=== READ DEVICE NAME (0x16) ===")
        resp = self.send_cmd(CMD_RD_DEV_NAME)
        if resp and len(resp) > 22 and resp[7] == CMD_RD_DEV_NAME:
            data_len = (resp[2] | (resp[3] << 8)) - 18
            if 0 < data_len <= len(resp) - 22:
                name = resp[22:22 + data_len].decode('ascii', errors='replace').rstrip('\x00')
                print(f"  -> Device name: {name}")
                return name
        return None

    def read_firmware_version(self):
        print("\n=== READ FIRMWARE VERSION (0xC5) ===")
        resp = self.send_cmd(CMD_READ_FWVER)
        if resp and len(resp) > 22 and resp[7] == (CMD_READ_FWVER & 0xFF):
            fw = resp[22]
            print(f"  -> Firmware version: {fw}")
            return fw
        return None

    def read_version(self):
        print("\n=== READ VERSION (0x17) ===")
        resp = self.send_cmd(CMD_READ_REV)
        if resp and len(resp) > 24 and resp[7] == CMD_READ_REV:
            ver = resp[22:25].decode('ascii', errors='replace')
            print(f"  -> Version: {ver}")
            return ver
        return None

    def start_print(self):
        print("\n=== START PRINT (0x13) ===")
        return self.send_cmd(CMD_START_PRINT)

    def stop_print(self):
        print("\n=== STOP PRINT (0x14) ===")
        return self.send_cmd(CMD_STOP_PRINT)


# ============================================================
# Print buffer construction
# ============================================================

def build_page_reg_bits(page_st=0, page_end=0, prt_end=0, cut=0,
                        savepaper=0, first_cut=0, nodu=4, mat=1):
    """Build 2-byte PAGE_REG_BITS (using toByteArray(6) as T50Plus does).

    Byte 0:
      bit 1: PageSt     bit 2: PageEnd
      bit 3: PrtEnd     bits 4-6: Cut (3 bits)
      bit 7: Savepaper

    Byte 1:
      bits 0-1: FirstCut      bits 2-5: Nodu (density)
      bits 6-7: Mat (shifted left by 6)
    """
    b0 = 0
    if page_st:  b0 |= 0x02
    if page_end: b0 |= 0x04
    if prt_end:  b0 |= 0x08
    b0 &= 0x0F  # IntersectionPtg.sid = 0x0F
    b0 |= (cut & 0x07) << 4
    b0 |= (savepaper & 0x01) << 7

    b1 = 0
    b1 |= (first_cut & 0x03)
    b1 |= (nodu & 0x0F) << 2
    b1 |= (mat & 0x03) << 6  # toByteArray(6) shifts Mat left by 6
    return bytes([b0, b1])


def build_print_buffer(image_data, per_line_byte, cols_in_buf,
                       page_st=False, page_end=False, prt_end=False,
                       margin_top=8, margin_bottom=8, density=4):
    """Build a 4096-byte print buffer.

    Buffer layout (from T50PlusPrint.initLZMAData):
      [0-1]   Checksum
      [2-3]   PAGE_REG_BITS
      [4-5]   Column count (little-endian)
      [6]     Bytes per line (per_line_byte)
      [7]     (reserved)
      [8-9]   Margin top (little-endian)
      [10-11] Margin bottom (little-endian)
      [12]    Red deepness
      [13]    0
      [14+]   Image data

    Checksum (bytes 0-1):
      base = sum of bytes[2..13]
      for each 256-byte boundary i: base += buf[(i*256)-1]
    """
    buf = bytearray(4096)

    # PAGE_REG_BITS
    page_bits = build_page_reg_bits(
        page_st=int(page_st), page_end=int(page_end),
        prt_end=int(prt_end), nodu=density, mat=1,
    )
    buf[2] = page_bits[0]
    buf[3] = page_bits[1]

    # Column count
    buf[4] = cols_in_buf & 0xFF
    buf[5] = (cols_in_buf >> 8) & 0xFF

    # Bytes per line
    buf[6] = per_line_byte & 0xFF

    # Margins
    mt = max(1, min(margin_top, 900))
    mb = max(1, min(margin_bottom, 900))
    buf[8]  = mt & 0xFF
    buf[9]  = (mt >> 8) & 0xFF
    buf[10] = mb & 0xFF
    buf[11] = (mb >> 8) & 0xFF

    # Red deepness
    buf[12] = min(density, 15)
    buf[13] = 0

    # Image data at offset 14
    data_len = min(len(image_data), 4096 - 14)
    buf[14:14 + data_len] = image_data[:data_len]

    # Checksum
    data_end = (cols_in_buf * per_line_byte) + 14
    chk = sum(buf[2:14])
    n_256 = data_end // 256
    for i in range(1, n_256 + 1):
        idx = (i * 256) - 1
        if idx < len(buf):
            chk += buf[idx] & 0xFF
    buf[0] = chk & 0xFF
    buf[1] = (chk >> 8) & 0xFF

    return bytes(buf)


# ============================================================
# Image creation
# ============================================================

def create_test_pattern(label_width_mm, height_mm, dpi=DPI):
    """Create a test pattern with buffer boundary markers.

    The Android app forces mMatWidth=48 for T50Plus (printhead width),
    creates a 384-dot-wide canvas, and centers the label content.
    After rotating -90 degrees, the image is column-major, LSB-first packed.

    Returns (image_bytes, canvas_width_dots, height_dots, bytes_per_line).
    """
    # T50Plus: canvas is always printhead width (48mm), not label width
    canvas_width_mm = PRINTHEAD_WIDTH_MM
    canvas_width_dots = canvas_width_mm * dpi  # 384
    height_dots = height_mm * dpi              # 240
    bytes_per_line = canvas_width_dots // 8    # 48

    # Label content area centered in canvas
    label_width_dots = label_width_mm * dpi   # 320
    x_offset = (canvas_width_dots - label_width_dots) // 2  # 32

    margin_top = 8
    margin_bottom = 8
    image_cols = height_dots - margin_top - margin_bottom
    max_cols = MAX_BUF_DATA // bytes_per_line  # 4074/48 = 84

    # Compute buffer regions (column ranges relative to full image)
    buf_regions = []
    col = margin_top
    while col < height_dots - margin_bottom:
        end = min(col + max_cols, height_dots - margin_bottom)
        buf_regions.append((col, end))
        col = end
    for i, (s, e) in enumerate(buf_regions):
        print(f"  Buffer {i}: columns {s}-{e} ({e-s} cols)")

    print(f"  Canvas: {canvas_width_dots}x{height_dots} dots "
          f"({bytes_per_line} bytes/line)")
    print(f"  Label area: {label_width_dots} dots, offset {x_offset}")

    # After -90 rotation: height_dots columns, each bytes_per_line wide
    buf = bytearray(bytes_per_line * height_dots)

    for col in range(height_dots):
        for row in range(canvas_width_dots):
            pixel = False

            # Only draw within the label area
            label_row = row - x_offset
            if 0 <= label_row < label_width_dots:
                # Outer border (2px)
                if (label_row < 2 or label_row >= label_width_dots - 2
                        or col < 2 or col >= height_dots - 2):
                    pixel = True

                # Per-buffer: border + X cross connecting corners
                for i, (bs, be) in enumerate(buf_regions):
                    if bs <= col < be:
                        bh = be - bs  # buffer height in cols
                        bw = label_width_dots
                        local_col = col - bs

                        # Buffer top/bottom border (2px thick)
                        if local_col < 2 or local_col >= bh - 2:
                            pixel = True

                        # X cross: two diagonals corner-to-corner (2px thick)
                        expected_row_1 = int(local_col * bw / bh)
                        if abs(label_row - expected_row_1) < 2:
                            pixel = True
                        expected_row_2 = bw - 1 - expected_row_1
                        if abs(label_row - expected_row_2) < 2:
                            pixel = True

                        # Buffer number: thick dots in top-left corner
                        for d in range(i + 1):
                            dx = 10 + d * 12
                            dy = 10
                            if (dx <= label_row < dx + 8
                                    and dy <= local_col < dy + 8):
                                pixel = True
                        break

            if pixel:
                byte_idx = col * bytes_per_line + (row // 8)
                bit_idx = row % 8  # LSB-first
                buf[byte_idx] |= (1 << bit_idx)

    return bytes(buf), canvas_width_dots, height_dots, bytes_per_line


# ============================================================
# Test print sequence
# ============================================================

def calc_speed(compressed_size):
    """Calculate print speed based on compressed buffer size.

    From T50PlusPrint.multiCompression(): the speed is derived from
    average compressed bytes per buffer.  Lower speed for larger data
    ensures the thermal head has enough time.
    """
    if compressed_size > 3000: return 10
    if compressed_size > 2800: return 15
    if compressed_size > 2500: return 20
    if compressed_size > 2000: return 25
    if compressed_size > 1500: return 40
    if compressed_size > 1000: return 45
    if compressed_size > 500:  return 55
    return 60


def do_test_print(printer, mat, diag=False):
    """Execute a test print following the T50PlusPrint.doPrint() flow."""
    label_width_mm = min(mat.get('width', MAX_WIDTH_MM), MAX_WIDTH_MM)
    height_mm = mat.get('height', 25)
    if height_mm == 0:
        height_mm = 25
    density = 4

    print(f"\n{'='*60}")
    print(f"  TEST PRINT: label {label_width_mm}mm x {height_mm}mm, "
          f"canvas {PRINTHEAD_WIDTH_MM}mm @ {DPI} dots/mm")
    print(f"  Density: {density}")
    print(f"{'='*60}")

    # Create test pattern (canvas is always printhead width = 48mm)
    image_data, w_dots, h_dots, per_line_byte = create_test_pattern(
        label_width_mm, height_mm, DPI
    )
    print(f"  Image: {w_dots}x{h_dots} dots, {per_line_byte} bytes/line")
    print(f"  Raw image data: {len(image_data)} bytes")

    # Split into 4096-byte print buffers
    # Android uses R2.drawable.sf5334_ = 4074 as max data area, NOT 4096-14
    margin_top = 8
    margin_bottom = 8
    max_cols = MAX_BUF_DATA // per_line_byte  # 4074/40 = 101 (not 102!)

    # Image columns = total height minus margins (Android: ColumnLeft -= margins)
    image_cols = h_dots - margin_top - margin_bottom
    print(f"  Image columns: {image_cols} (margins: {margin_top}+{margin_bottom})")

    cols_remaining = image_cols
    current_col = 0  # column offset into the image data
    raw_buffers = []

    while cols_remaining > 0:
        cols_in_buf = min(cols_remaining, max_cols)
        is_first = (current_col == 0)
        is_last = (cols_remaining <= max_cols)

        # Image data offset: skip margin_top columns at the start
        # (Android: CurrentFrame += margin_top before GetBytes)
        img_start = (margin_top + current_col) * per_line_byte
        img_end = img_start + cols_in_buf * per_line_byte
        img_chunk = image_data[img_start:img_end]

        if diag:
            # DIAG mode: each buffer is a self-contained single-buffer print
            buf = build_print_buffer(
                img_chunk, per_line_byte, cols_in_buf,
                page_st=True, page_end=True, prt_end=True,
                margin_top=margin_top, margin_bottom=margin_bottom,
                density=density,
            )
        else:
            buf = build_print_buffer(
                img_chunk, per_line_byte, cols_in_buf,
                page_st=is_first, page_end=is_last, prt_end=is_last,
                margin_top=margin_top, margin_bottom=margin_bottom,
                density=density,
            )
        raw_buffers.append(buf)
        current_col += cols_in_buf
        cols_remaining -= cols_in_buf

    print(f"  Print buffers: {len(raw_buffers)}")
    if diag:
        print(f"  DIAGNOSTIC MODE: each buffer sent as separate print job")

    # LZMA parameters from the Android app
    # (LzmaUtils.java: dict=8192, lc=3, lp=0, pb=2, algo=2, mf=BT4, fb=128)
    # The printer firmware has limited RAM - large dictionaries will fail!
    lzma_filters = [{
        'id':        lzma_mod.FILTER_LZMA1,
        'dict_size': 8192,       # 8KB dictionary (critical for printer firmware)
        'lc':        3,          # literal context bits
        'lp':        0,          # literal position bits
        'pb':        2,          # position bits
        'mode':      lzma_mod.MODE_NORMAL,
        'nice_len':  128,        # fast bytes
        'mf':        lzma_mod.MF_BT4,
    }]

    # Dump individual buffer headers for debugging
    for i, buf in enumerate(raw_buffers):
        print(f"  Buffer {i}: {len(buf)} bytes, hdr: {buf[:14].hex()}")

    # KEY INSIGHT from JS Electron app (initLZMAData):
    # All raw 4096-byte buffers are CONCATENATED into one array, then
    # LZMA-compressed as a single stream and sent in one transfer.
    # The printer firmware decompresses and processes each 4096-byte block.
    concat_data = b''.join(raw_buffers)
    print(f"  Concatenated: {len(concat_data)} bytes ({len(raw_buffers)} x 4096)")

    compressed_all = lzma_mod.compress(
        concat_data, format=lzma_mod.FORMAT_ALONE, filters=lzma_filters
    )
    # Patch LZMA header: uncompressed size must be exact (not -1)
    compressed_all = bytearray(compressed_all)
    struct.pack_into('<Q', compressed_all, 5, len(concat_data))
    compressed_all = bytes(compressed_all)

    # Speed from average compressed size per buffer
    # (Java multiCompression: r0 = r3.length / r10, then speed thresholds)
    avg_compressed = len(compressed_all) / len(raw_buffers)
    print_speed = calc_speed(int(avg_compressed))
    print(f"  Compressed: {len(compressed_all)} bytes, "
          f"avg={avg_compressed:.0f}/buf, speed={print_speed}")
    print(f"  LZMA hdr: {compressed_all[:13].hex()}")

    # For diag mode, also prepare per-buffer compressed data
    if diag:
        compressed_buffers = []
        speeds = []
        min_speed = 60
        for i, buf in enumerate(raw_buffers):
            compressed = lzma_mod.compress(
                buf, format=lzma_mod.FORMAT_ALONE, filters=lzma_filters
            )
            compressed = bytearray(compressed)
            struct.pack_into('<Q', compressed, 5, len(buf))
            compressed = bytes(compressed)
            compressed_buffers.append(compressed)
            buf_speed = calc_speed(len(compressed))
            min_speed = min(min_speed, buf_speed)
            speeds.append(min_speed)

    # === PRINT SEQUENCE (from T50PlusPrint.doPrint) ===

    def _transfer_one_buffer(compressed, buf_speed):
        """Transfer a single compressed buffer: DMA start + data + BUF_FULL."""
        num_packets = (len(compressed) + 499) // 500
        print(f"  Compressed: {len(compressed)} bytes, {num_packets} packets")

        # DMA start
        print("  Starting DMA transfer...")
        resp = printer.send_cmd_start_trans(CMD_NEXT_ZIPPEDBULK, 512, num_packets)
        if not resp:
            print("  Start transfer failed!")
            return False

        # Data packets
        for pkt_idx in range(num_packets):
            offset = pkt_idx * 500
            chunk = compressed[offset:offset + 500]
            data_pkt = make_data_packet(chunk, pkt_idx, num_packets)
            print(f"  Sending packet {pkt_idx + 1}/{num_packets} "
                  f"({len(chunk)} bytes)...")
            read_resp = (pkt_idx == num_packets - 1)
            resp = printer.send_data_frame(data_pkt, read_response=read_resp)

        # 20ms delay after last data packet
        time.sleep(0.02)

        # BUF_FULL
        print(f"  Sending BUF_FULL (len={len(compressed)}, speed={buf_speed})...")
        resp = printer.send_cmd_start_trans(CMD_BUF_FULL, len(compressed), buf_speed)
        if resp:
            print(f"  BUF_FULL resp: {fmt_hex(resp)}")
        else:
            print(f"  WARNING: No response to BUF_FULL!")
        return True

    def _wait_ready():
        """Wait for device to be idle."""
        for attempt in range(60):
            status = printer.query_status()
            if not status:
                return None
            if not status.get('device_busy') and not status.get('printing'):
                return status
            time.sleep(0.1)
        return None

    def _wait_printing():
        """Wait for printing station to become active."""
        for attempt in range(60):
            status = printer.query_status()
            if status and status.get('printing'):
                return status
            time.sleep(0.1)
        return None

    if diag:
        # ---- DIAGNOSTIC MODE: each buffer as its own print job ----
        for buf_idx, compressed in enumerate(compressed_buffers):
            print(f"\n{'='*60}")
            print(f"  DIAG: Printing buffer {buf_idx} as standalone job")
            print(f"{'='*60}")

            # Wait for device ready
            status = _wait_ready()
            if not status:
                print("  Timeout waiting for device!")
                return False

            # Start print
            printer.start_print()
            status = _wait_printing()
            if not status:
                print("  Timeout waiting for printing station!")
                return False

            # Transfer
            try:
                _transfer_one_buffer(compressed, speeds[buf_idx])
            except (OSError,) as e:
                print(f"  Transfer error: {e}")
                return False

            # Wait for this job to complete
            print("  Waiting for job completion...")
            for attempt in range(300):
                time.sleep(0.1)
                status = printer.query_status()
                if status and not status.get('printing') and not status.get('device_busy'):
                    print(f"  Job {buf_idx} complete!")
                    break
            else:
                print(f"  Timeout on job {buf_idx}.")

            time.sleep(1)  # gap between jobs

        print("\nAll diagnostic jobs done.")
        return True

    # ---- NORMAL MODE: multi-buffer print ----

    # Step 1: Check device
    print("\n--- Step 1: CHECK_DEVICE ---")
    if not printer.check_device():
        print("CHECK_DEVICE failed!")
        return False

    # Step 2: Wait for device ready
    print("\n--- Step 2: Wait for device ready ---")
    status = _wait_ready()
    if not status:
        print("  Timeout waiting for device!")
        return False
    print("  Device ready.")

    # Check for errors
    for err_key in ['label_rw_error', 'label_mode_error', 'label_end',
                    'cover_open', 'head_temp_high', 'label_not_installed']:
        if status.get(err_key):
            print(f"  ERROR: {err_key} is set!")
            return False

    # Step 3: Start print
    print("\n--- Step 3: START_PRINT ---")
    resp = printer.start_print()
    if not resp:
        print("START_PRINT failed!")
        return False

    # Step 4: Wait for printing station active
    print("\n--- Step 4: Wait for printing station ---")
    status = _wait_printing()
    if not status:
        print("  Timeout waiting for printing station!")
        return False
    print("  Printing station active.")

    # Step 5: Transfer single compressed chunk (all buffers concatenated)
    print(f"\n--- Step 5: Transfer concatenated compressed data ---")

    # Wait for buffer available (BufSta == 0)
    for wait_i in range(200):
        time.sleep(0.02)
        try:
            status = printer.query_status()
        except (OSError,) as e:
            print(f"  Connection error during status query: {e}")
            return False
        if status and not status.get('buf_full'):
            print(f"  Buffer ready (printing={status.get('printing')}, "
                  f"busy={status.get('device_busy')})")
            break
        if wait_i % 10 == 0 and wait_i > 0:
            print(f"  Waiting for buffer space... ({wait_i})")
    else:
        print("  Timeout waiting for buffer space!")
        return False

    # Check for errors
    if status:
        for ek in ['label_rw_error', 'cover_open', 'head_temp_high',
                    'label_not_installed']:
            if status.get(ek):
                print(f"  ERROR: {ek}!")
                printer.stop_print()
                return False

    try:
        _transfer_one_buffer(compressed_all, print_speed)
    except (OSError,) as e:
        print(f"  Transfer error: {e}")
        return False

    # Step 6: Wait for print completion
    print("\n--- Step 6: Wait for completion ---")
    for attempt in range(300):
        time.sleep(0.1)
        try:
            status = printer.query_status()
        except OSError:
            print("  Connection lost during completion wait.")
            return True  # Print may have completed
        if status:
            if not status.get('printing') and not status.get('device_busy'):
                print("  Print complete!")
                return True

    print("  Timeout waiting for completion.")
    return True


# ============================================================
# Main
# ============================================================

def main():
    target = None  # Auto-detect: try BT socket, fallback to /dev/rfcomm0
    do_print = False

    diag_mode = False
    for arg in sys.argv[1:]:
        if arg == '--print':
            do_print = True
        elif arg == '--diag':
            do_print = True
            diag_mode = True
        elif not arg.startswith('-'):
            target = arg

    printer = SupvanPrinter(target=target)

    try:
        printer.connect()

        # Probe the printer
        print("\n" + "=" * 60)
        print("  PROBING PRINTER")
        print("=" * 60)

        if not printer.check_device():
            print("\nNo response to CHECK_DEVICE (0x7E 0x5A format).")
            print("Trying short 0xC0 0x40 format as fallback...")
            # Fallback: try the 8-byte 0xC0 0x40 format
            pkt = bytes([0xC0, 0x40, 0x00, 0x00,
                         CMD_CHECK_DEVICE, 0x00, 0x08, 0x00])
            print(f"  TX: {pkt.hex()}")
            printer._write_chunked(pkt)
            resp = printer._read_response()
            if resp:
                print(f"  RX: {fmt_hex(resp)}")
                print("  -> Short format works!")
            else:
                print("  -> Still no response. Printer may not be connected.")
                print("\nTroubleshooting:")
                print("  1. Is the printer powered on?")
                print("  2. Is it paired? bluetoothctl info A4:93:40:A0:87:57")
                return

        print("\n*** Printer is responding! ***")

        status = printer.query_status()
        printer.read_device_name()
        printer.read_firmware_version()
        printer.read_version()
        mat = printer.query_material()

        if mat:
            print(f"\n{'='*60}")
            print(f"  MATERIAL SUMMARY")
            print(f"{'='*60}")
            print(f"  Width:     {mat.get('width', '?')} mm")
            print(f"  Height:    {mat.get('height', '?')} mm")
            print(f"  Type:      {mat.get('type', '?')}")
            print(f"  SN:        {mat.get('sn', '?')}")
            print(f"  Gap:       {mat.get('gap', '?')}")
            print(f"  UUID:      {mat.get('uuid', '?')}")
            print(f"  Code:      {mat.get('code', '?')}")
            if 'remind' in mat:
                print(f"  Remaining: {mat['remind']}")
            if 'device_sn' in mat:
                print(f"  Device SN: {mat['device_sn']}")

        print(f"\n{'='*60}")
        print(f"  PROBE COMPLETE")
        print(f"{'='*60}")

        if do_print:
            if not mat:
                print("\nNo material info available, using defaults "
                      f"({MAX_WIDTH_MM}mm x 25mm)")
                mat = {'width': MAX_WIDTH_MM, 'height': 25, 'type': 0}
            do_test_print(printer, mat, diag=diag_mode)
        else:
            print("\nTo test print, run again with --print flag.")

    except serial.SerialException as e:
        print(f"\nSerial error: {e}")
    except OSError as e:
        print(f"\nI/O error (connection may have dropped): {e}")
    except KeyboardInterrupt:
        print("\nInterrupted.")
    finally:
        try:
            printer.close()
        except Exception:
            pass


if __name__ == '__main__':
    main()
