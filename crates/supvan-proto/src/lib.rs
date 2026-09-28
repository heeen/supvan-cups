//! Supvan T-series wire protocol — the reusable, transport-agnostic core.
//!
//! Speaks the framed command/response protocol the printers use over both
//! Bluetooth RFCOMM (`0x7E5A` framing, little-endian params) and USB HID
//! (`0xC040` framing, big-endian params), behind a single [`transport::Transport`]
//! trait. On top of that it provides the print pipeline: 1-bit bitmap packing
//! into the printhead's column-major layout ([`bitmap`]), LZMA1-"alone"
//! compression of the print buffers ([`compress`]), status and loaded-material
//! decoding ([`status`]), and the high-level print flow ([`printer::Printer`]).
//!
//! This crate has no IPP/CUPS knowledge — `supvan-app` layers that on via the
//! `ipp-printer-app` framework. See `docs/PROTOCOL.md` for the wire format.

pub mod bitmap;
pub mod ble;
pub mod buffer;
pub mod cmd;
pub mod compress;
pub mod data;
pub mod dither;
pub mod error;
pub mod hidraw;
pub mod printer;
pub mod profile;
pub mod rfcomm;
pub mod rfid;
pub mod speed;
pub mod spp_pipe;
pub mod status;
pub mod transport;
pub mod twocolor;
pub mod usb_transport;
