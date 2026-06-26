//! Dump raw RETURN_MAT responses from USB and BT side by side, then locate
//! the printer's known serial substring in both buffers.
//!
//! Usage:
//!   sudo systemctl --user stop supvan-printer-app
//!   cargo run --release -p supvan-cli --example material_probe -- \
//!     /dev/hidraw8 A4:93:40:A0:87:57 T0117A2410211517
//!   sudo systemctl --user start supvan-printer-app
//!
//! Expect one BT beep on dial (we open a fresh RFCOMM socket).

use std::env;

use supvan_proto::cmd::CMD_RETURN_MAT;
use supvan_proto::hidraw::HidrawDevice;
use supvan_proto::rfcomm::RfcommSocket;
use supvan_proto::spp_pipe::SppCodec;
use supvan_proto::transport::Transport;
use supvan_proto::usb_transport::UsbHidTransport;

fn hexdump(label: &str, bytes: &[u8]) {
    println!("[{label}] {} bytes:", bytes.len());
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let off = i * 16;
        let hex: String = chunk
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {off:3}:  {hex:<48}  {ascii}");
    }
}

fn find_serial(label: &str, bytes: &[u8], needle: &str) {
    let needle_bytes = needle.as_bytes();
    if let Some(pos) = bytes
        .windows(needle_bytes.len())
        .position(|w| w == needle_bytes)
    {
        println!("[{label}] serial '{needle}' found as ASCII at offset {pos}");
    } else {
        println!("[{label}] serial '{needle}' NOT found as ASCII");
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: material_probe <hidraw> <bt_mac> <known_serial>");
        std::process::exit(2);
    }
    let hidraw_path = &args[1];
    let bt_mac = &args[2];
    let serial = &args[3];

    let usb_dev = HidrawDevice::open(hidraw_path).expect("hidraw open");
    let usb_t = UsbHidTransport::new(usb_dev);

    println!("(dialing BT — expect one beep)");
    let sock = RfcommSocket::connect_default(bt_mac).expect("rfcomm connect");
    let bt_t = SppCodec::new(sock);

    let usb_resp = usb_t
        .send_cmd(CMD_RETURN_MAT, 0)
        .await
        .expect("usb send")
        .unwrap_or_default();
    let bt_resp = bt_t
        .send_cmd(CMD_RETURN_MAT, 0)
        .await
        .expect("bt send")
        .unwrap_or_default();

    hexdump("USB", &usb_resp);
    hexdump("BT ", &bt_resp);

    println!();
    find_serial("USB", &usb_resp, serial);
    find_serial("BT ", &bt_resp, serial);
}
