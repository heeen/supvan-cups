# supvan-proto

The transport-agnostic wire-protocol layer for Supvan T-series thermal label
printers. No IPP, CUPS, or async — just the bytes on the link. `supvan-app`
builds the IPP Everywhere printer on top of this; `supvan-cli` uses it directly.

What it provides:

- **Two transports behind one `Transport` trait**: Bluetooth RFCOMM (`0x7E5A`
  framing, little-endian params) and USB HID (`0xC040` framing, big-endian
  params, fixed-size responses). The same command codes ride both (`cmd`).
- **Status & material decoding** (`status`): `INQUIRY_STA` printer-state bits
  and the `RETURN_MAT` loaded-label / RFID / remaining-count report, on both
  transports.
- **Print pipeline**: 1-bit raster → printhead column-major packing (`bitmap`),
  4096-byte buffer assembly (`buffer`), LZMA1-"alone" compression with the
  firmware's exact parameters (`compress`), and the high-level send sequence
  (`printer::Printer`).

The on-the-wire format — frame layouts, command table, status bit assignments,
and the per-transport asymmetries — is documented in
[`../../docs/PROTOCOL.md`](../../docs/PROTOCOL.md). The Rust source is always
the more authoritative reference.

Not published to crates.io; it's a workspace member of
[supvan-cups](https://github.com/heeen/supvan-cups).
