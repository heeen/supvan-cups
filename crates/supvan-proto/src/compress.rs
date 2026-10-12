use crate::buffer::PRINT_BUF_SIZE;
use crate::error::{Error, Result};
use crate::speed::calc_speed;

/// Largest compressed block the firmware will accept in one transfer — the
/// size of its receive buffer.
const MAX_COMPRESSED_BLOCK: usize = PRINT_BUF_SIZE;

/// Maximum print buffers per LZMA block on the Android/SPP path —
/// `bufferMAXCount` in `T50PlusPrint.multiCompression()`.
const MAX_GROUP_BUFFERS: usize = 4;

/// Electron's cut-over (`getEncodeData`): when the buffers, compressed one at
/// a time, sum past this, every buffer goes out as its own block.
const PER_BUFFER_THRESHOLD: usize = 4000;

/// Electron's fixed speeds: 20 for a page split over several blocks, 60 for a
/// single-block page (`t5080PrintUtils.doSupVanPrint`).
const SPLIT_PAGE_SPEED: u16 = 20;
const SINGLE_BLOCK_SPEED: u16 = 60;

/// Measured Classic-BT cost of moving one block to the printer: each 512-byte
/// packet is written as 4 × 128 B with 10 ms gaps and waits for its ack
/// (~50 ms all told), plus NEXT_ZIPPEDBULK, BUF_FULL and a status poll.
const SPP_PACKET_MS: u32 = 50;
const SPP_BLOCK_OVERHEAD_MS: u32 = 180;
/// Run the head at most this fraction of what the link can deliver, so link
/// jitter doesn't starve it.
const SPP_LINK_HEADROOM: f32 = 0.85;
/// Speeds the vendor itself sends — `calc_speed`'s table plus the app's photo
/// override of 5. The speed value tracks feed rate in roughly mm/s.
const VENDOR_SPEEDS: [u16; 9] = [60, 55, 45, 40, 25, 20, 15, 10, 5];

/// LZMA1 parameters both vendor encoders use (Android `LzmaUtils`).
const DICT_SIZE: u32 = 8192;
const LC: u32 = 3;
const LP: u32 = 0;
const PB: u32 = 2;
const NICE_LEN: u32 = 128;

/// LZMA1 with an out-of-band size and no end-of-payload marker
/// (`LZMA_FILTER_LZMA1EXT` in `lzma/lzma12.h`; liblzma-sys doesn't export it).
const LZMA_FILTER_LZMA1EXT: u64 = 0x4000_0000_0000_0002;

/// How print buffers are grouped into LZMA blocks. The two vendor tools
/// disagree, and each matches the transport it drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// Android over SPP/BLE (`multiCompression`): up to four buffers per block,
    /// dropping one at a time until the block fits; the page runs at the
    /// minimum of `calc_speed` over its blocks.
    Grouped,
    /// Electron over USB HID (`getEncodeData`): the whole page in one block
    /// when it compresses small, otherwise one block per buffer at speed 20.
    PerBuffer,
}

/// One LZMA block ready for transfer.
#[derive(Debug, Clone)]
pub struct CompressedBlock {
    pub data: Vec<u8>,
    /// How many 4096-byte print buffers were concatenated before compression.
    pub source_buffers: u8,
    /// Sent as BUF_FULL's second parameter.
    pub speed: u16,
}

/// Compress to an LZMA1 "alone" stream the firmware accepts: the 13-byte
/// header (props `0x5D`, dict 8192, exact uncompressed size) followed by a
/// payload with **no end-of-payload marker**.
///
/// Both vendor encoders omit the marker — Android's SDK encoder runs with
/// `EndMarker = false`, and Supvan patched their lzma-js copy to force
/// `_writeEndMark = false`. liblzma's `.lzma` encoder always writes one, and
/// the firmware chokes on it: the last buffer of every block went missing and
/// a page split over several blocks stopped at the first block boundary.
pub fn compress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    use liblzma_sys as sys;

    // SAFETY: lzma_options_lzma and lzma_stream are plain C structs for which
    // all-zero is a valid initial state (LZMA_STREAM_INIT is all zeros).
    let mut opts: sys::lzma_options_lzma = unsafe { std::mem::zeroed() };
    // SAFETY: `opts` is a valid, writable options struct.
    if unsafe { sys::lzma_lzma_preset(&mut opts, 6) } != 0 {
        return Err(Error::Compression("lzma preset 6 unsupported".into()));
    }
    opts.dict_size = DICT_SIZE;
    opts.lc = LC;
    opts.lp = LP;
    opts.pb = PB;
    opts.nice_len = NICE_LEN;
    // ext_flags = 0 (no LZMA_LZMA1EXT_ALLOW_EOPM): the encoder writes no marker.
    opts.ext_flags = 0;

    let filters = [
        sys::lzma_filter {
            id: LZMA_FILTER_LZMA1EXT,
            options: (&raw mut opts).cast(),
        },
        sys::lzma_filter {
            id: sys::LZMA_VLI_UNKNOWN,
            options: std::ptr::null_mut(),
        },
    ];

    let mut out = Vec::with_capacity(13 + data.len() + data.len() / 2 + 64);
    out.push((PB * 5 + LP) as u8 * 9 + LC as u8);
    out.extend_from_slice(&DICT_SIZE.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());

    // SAFETY: see above.
    let mut strm: sys::lzma_stream = unsafe { std::mem::zeroed() };
    // SAFETY: `strm` is zero-initialised and `filters` is terminated by
    // LZMA_VLI_UNKNOWN; `opts` outlives the encoder (it ends below).
    let ret = unsafe { sys::lzma_raw_encoder(&mut strm, filters.as_ptr()) };
    if ret != sys::LZMA_OK {
        return Err(Error::Compression(format!("lzma_raw_encoder: {ret}")));
    }

    strm.next_in = data.as_ptr();
    strm.avail_in = data.len();
    let result = loop {
        if out.capacity() == out.len() {
            out.reserve(4096);
        }
        let spare = out.spare_capacity_mut();
        strm.next_out = spare.as_mut_ptr().cast();
        strm.avail_out = spare.len();
        let before = strm.avail_out;
        // SAFETY: next_in/next_out point into live buffers sized by avail_*.
        let ret = unsafe { sys::lzma_code(&mut strm, sys::LZMA_FINISH) };
        let written = before - strm.avail_out;
        // SAFETY: lzma_code initialised exactly `written` bytes of spare capacity.
        unsafe { out.set_len(out.len() + written) };
        match ret {
            sys::LZMA_STREAM_END => break Ok(()),
            sys::LZMA_OK => {}
            other => break Err(Error::Compression(format!("lzma_code: {other}"))),
        }
    };
    // SAFETY: `strm` was initialised by lzma_raw_encoder.
    unsafe { sys::lzma_end(&mut strm) };
    result.map(|()| out)
}

/// Decompress a stream produced by [`compress_lzma`] (or by a vendor tool).
pub fn decompress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    use liblzma::stream::Stream;
    use std::io::Read;

    let stream = Stream::new_lzma_decoder(u64::MAX)
        .map_err(|e| Error::Compression(format!("decoder: {e}")))?;
    let mut decoder = liblzma::read::XzDecoder::new_stream(data, stream);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Compression(format!("decompress: {e}")))?;
    Ok(out)
}

/// Split a page's print buffers into LZMA blocks, one per NEXT_ZIPPEDBULK
/// transfer, the way the vendor tool for the given transport does.
pub fn compress_buffers(
    buffers: &[[u8; PRINT_BUF_SIZE]],
    packing: Packing,
) -> Result<Vec<CompressedBlock>> {
    if buffers.is_empty() {
        return Err(Error::InvalidParam("no buffers to compress".into()));
    }
    let blocks = match packing {
        Packing::Grouped => compress_grouped(buffers)?,
        Packing::PerBuffer => compress_per_buffer(buffers)?,
    };
    log::debug!(
        "compress ({packing:?}): {} buffers -> {:?}",
        buffers.len(),
        blocks
            .iter()
            .map(|b| (b.data.len(), b.source_buffers, b.speed))
            .collect::<Vec<_>>()
    );
    Ok(blocks)
}

/// Android `multiCompression`.
fn compress_grouped(buffers: &[[u8; PRINT_BUF_SIZE]]) -> Result<Vec<CompressedBlock>> {
    let mut blocks = Vec::new();
    let mut page_speed = SINGLE_BLOCK_SPEED;
    let mut rest = buffers;
    while !rest.is_empty() {
        let mut take = MAX_GROUP_BUFFERS.min(rest.len());
        let data = loop {
            let z = compress_lzma(&rest[..take].concat())?;
            if z.len() <= MAX_COMPRESSED_BLOCK {
                break z;
            }
            if take == 1 {
                return Err(oversized(z.len()));
            }
            take -= 1;
        };
        page_speed = page_speed
            .min(calc_speed(data.len() / take))
            .min(spp_link_speed(data.len(), &rest[..take]));
        blocks.push(CompressedBlock {
            data,
            source_buffers: take as u8,
            speed: page_speed,
        });
        rest = &rest[take..];
    }
    // Android's running minimum only ever falls, but it is computed while the
    // page is still being encoded, so early blocks can go out faster than
    // later ones. Changing feed speed mid-label leaves a visible seam; run the
    // whole page at its slowest block's speed.
    //
    // `calc_speed` alone is not enough over Classic BT: a dense halftone
    // buffer (~2.5 KB for 10.5 mm) takes ~0.43 s to deliver, ~24 mm/s, and
    // `calc_speed` picks 25 for it — the head outruns the link and stutters,
    // leaving stripes. The vendor app dodges this by forcing speed 5 for
    // photos; capping by the link's actual delivery rate is the general form.
    for b in &mut blocks {
        b.speed = page_speed;
    }
    Ok(blocks)
}

/// Fastest vendor speed the Classic-BT link can feed for this block.
fn spp_link_speed(compressed_len: usize, buffers: &[[u8; PRINT_BUF_SIZE]]) -> u16 {
    let feed_cols: u32 = buffers.iter().map(feed_cols).sum();
    let packets = compressed_len.div_ceil(crate::data::DATA_PAYLOAD_SIZE) as u32;
    let deliver_ms = packets * SPP_PACKET_MS + SPP_BLOCK_OVERHEAD_MS;
    let mm_per_s = feed_cols as f32 / crate::bitmap::DOTS_PER_MM as f32 * 1000.0
        / deliver_ms as f32
        * SPP_LINK_HEADROOM;
    VENDOR_SPEEDS
        .into_iter()
        .find(|&s| f32::from(s) <= mm_per_s)
        .unwrap_or(VENDOR_SPEEDS[VENDOR_SPEEDS.len() - 1])
}

/// Printed feed columns in one buffer: the header's column count, halved in
/// two-colour mode (`first_cut == 2`), which ships each column twice.
fn feed_cols(buf: &[u8; PRINT_BUF_SIZE]) -> u32 {
    let cols = u32::from(u16::from_le_bytes([buf[4], buf[5]]));
    if buf[3] & 0x03 == 2 { cols / 2 } else { cols }
}

/// Electron `getEncodeData`.
fn compress_per_buffer(buffers: &[[u8; PRINT_BUF_SIZE]]) -> Result<Vec<CompressedBlock>> {
    let singles = buffers
        .iter()
        .map(|b| compress_lzma(b))
        .collect::<Result<Vec<_>>>()?;
    if singles.iter().map(Vec::len).sum::<usize>() <= PER_BUFFER_THRESHOLD {
        let whole = compress_lzma(&buffers.concat())?;
        if whole.len() <= MAX_COMPRESSED_BLOCK
            && let Ok(source_buffers) = u8::try_from(buffers.len())
        {
            return Ok(vec![CompressedBlock {
                data: whole,
                source_buffers,
                speed: SINGLE_BLOCK_SPEED,
            }]);
        }
    }
    let speed = if singles.len() > 1 {
        SPLIT_PAGE_SPEED
    } else {
        SINGLE_BLOCK_SPEED
    };
    singles
        .into_iter()
        .map(|data| {
            if data.len() > MAX_COMPRESSED_BLOCK {
                return Err(oversized(data.len()));
            }
            Ok(CompressedBlock {
                data,
                source_buffers: 1,
                speed,
            })
        })
        .collect()
}

fn oversized(len: usize) -> Error {
    Error::Compression(format!(
        "print buffer compresses to {len} bytes, past the \
         {MAX_COMPRESSED_BLOCK}-byte receive buffer"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Density, PageOptions, split_into_buffers};

    fn noise_buffer(seed: u32) -> [u8; PRINT_BUF_SIZE] {
        let mut s = seed;
        let mut b = [0u8; PRINT_BUF_SIZE];
        for byte in b.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (s >> 24) as u8;
        }
        b
    }

    /// An 80 mm label: 639 feed columns, eight print buffers. `noisy` makes
    /// every byte depend on its column so the page cannot fit one block.
    fn eighty_mm_buffers(noisy: bool) -> Vec<[u8; PRINT_BUF_SIZE]> {
        let cols: u16 = 639;
        let bpl: u8 = 48;
        let mut s = 0x9e37_79b9u32;
        let canvas: Vec<u8> = (0..cols as usize * bpl as usize)
            .map(|i| {
                if noisy {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    // Sparse noise: compressible, but not into one block.
                    ((s >> 24) as u8) & ((s >> 16) as u8) & 0x5a
                } else {
                    ((i / bpl as usize) % 251) as u8
                }
            })
            .collect();
        let buffers = split_into_buffers(
            &canvas,
            bpl,
            cols,
            0,
            0,
            Density::uniform(4),
            PageOptions::default(),
        );
        assert_eq!(buffers.len(), 8);
        buffers
    }

    fn roundtrip(blocks: &[CompressedBlock]) -> Vec<u8> {
        blocks
            .iter()
            .flat_map(|b| {
                let raw = decompress_lzma(&b.data).unwrap();
                assert_eq!(raw.len(), usize::from(b.source_buffers) * PRINT_BUF_SIZE);
                raw
            })
            .collect()
    }

    #[test]
    fn header_carries_props_dict_and_exact_size() {
        let z = compress_lzma(&[0u8; 4096]).unwrap();
        assert_eq!(z[0], 0x5D, "lc=3 lp=0 pb=2");
        assert_eq!(&z[1..5], &8192u32.to_le_bytes());
        assert_eq!(&z[5..13], &4096u64.to_le_bytes());
    }

    /// The firmware wants what the vendor encoders produce: no end marker.
    /// A strict LZMA1EXT decoder, told the size and *not* allowed a marker,
    /// rejects a stream that has one.
    #[test]
    fn stream_has_no_end_marker() {
        use liblzma_sys as sys;

        let data: Vec<u8> = (0..3 * PRINT_BUF_SIZE).map(|i| (i % 7) as u8).collect();
        let z = compress_lzma(&data).unwrap();

        // SAFETY: zeroed C structs; filters terminated; buffers sized below.
        unsafe {
            let mut opts: sys::lzma_options_lzma = std::mem::zeroed();
            assert_eq!(sys::lzma_lzma_preset(&mut opts, 6), 0);
            opts.dict_size = DICT_SIZE;
            opts.ext_flags = 0;
            opts.ext_size_low = data.len() as u32;
            opts.ext_size_high = 0;
            let filters = [
                sys::lzma_filter {
                    id: LZMA_FILTER_LZMA1EXT,
                    options: (&raw mut opts).cast(),
                },
                sys::lzma_filter {
                    id: sys::LZMA_VLI_UNKNOWN,
                    options: std::ptr::null_mut(),
                },
            ];
            let mut strm: sys::lzma_stream = std::mem::zeroed();
            assert_eq!(
                sys::lzma_raw_decoder(&mut strm, filters.as_ptr()),
                sys::LZMA_OK
            );
            let mut out = vec![0u8; data.len() + 64];
            strm.next_in = z[13..].as_ptr();
            strm.avail_in = z.len() - 13;
            strm.next_out = out.as_mut_ptr();
            strm.avail_out = out.len();
            let ret = sys::lzma_code(&mut strm, sys::LZMA_FINISH);
            let produced = out.len() - strm.avail_out;
            let leftover = strm.avail_in;
            sys::lzma_end(&mut strm);
            assert_eq!(ret, sys::LZMA_STREAM_END, "strict no-marker decode");
            assert_eq!(leftover, 0, "trailing bytes after the payload");
            assert_eq!(&out[..produced], &data[..]);
        }
    }

    #[test]
    fn roundtrips_through_the_alone_decoder() {
        let data = vec![0x42u8; 1024];
        assert_eq!(
            decompress_lzma(&compress_lzma(&data).unwrap()).unwrap(),
            data
        );
    }

    #[test]
    fn grouped_caps_blocks_at_four_buffers() {
        let buffers = vec![[0u8; PRINT_BUF_SIZE]; 9];
        let blocks = compress_buffers(&buffers, Packing::Grouped).unwrap();
        let sizes: Vec<u8> = blocks.iter().map(|b| b.source_buffers).collect();
        assert_eq!(sizes, [4, 4, 1]);
        assert_eq!(roundtrip(&blocks), buffers.concat());
    }

    #[test]
    fn grouped_page_runs_at_its_slowest_block_speed() {
        // Dense first group, empty tail: the tail inherits the dense speed.
        let mut buffers: Vec<_> = (0..4).map(noise_buffer).collect();
        for b in &mut buffers {
            // Halve the entropy so a single buffer still fits a block.
            for (i, byte) in b.iter_mut().enumerate() {
                if i % 2 == 0 {
                    *byte = 0;
                }
            }
        }
        buffers.push([0u8; PRINT_BUF_SIZE]);
        let blocks = compress_buffers(&buffers, Packing::Grouped).unwrap();
        let slowest = blocks
            .iter()
            .map(|b| calc_speed(b.data.len() / usize::from(b.source_buffers)))
            .min()
            .unwrap();
        assert!(blocks.iter().all(|b| b.speed == slowest));
    }

    /// A dense 10.5 mm buffer (~2.5 KB) can't be fed over BT at the 25 that
    /// `calc_speed` picks; blank ones arrive fast enough for full speed.
    #[test]
    fn link_cap_slows_dense_bt_pages_only() {
        let mut dense = [0u8; PRINT_BUF_SIZE];
        dense[4] = 84; // full 48-byte-line buffer
        assert_eq!(spp_link_speed(2480, &[dense]), 20);
        assert_eq!(spp_link_speed(2806, &[dense, dense]), 25);
        assert_eq!(spp_link_speed(120, &[dense; 4]), 60);
    }

    #[test]
    fn per_buffer_keeps_a_small_page_in_one_block_at_speed_60() {
        let buffers = eighty_mm_buffers(false);
        let blocks = compress_buffers(&buffers, Packing::PerBuffer).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source_buffers, 8);
        assert_eq!(blocks[0].speed, 60);
        assert_eq!(roundtrip(&blocks), buffers.concat());
    }

    #[test]
    fn per_buffer_splits_a_dense_page_one_buffer_per_block_at_speed_20() {
        let buffers = eighty_mm_buffers(true);
        let blocks = compress_buffers(&buffers, Packing::PerBuffer).unwrap();
        assert_eq!(blocks.len(), 8);
        assert!(
            blocks
                .iter()
                .all(|b| b.source_buffers == 1 && b.speed == 20)
        );
        assert_eq!(roundtrip(&blocks), buffers.concat());
    }

    #[test]
    fn incompressible_buffer_is_refused_by_both() {
        for packing in [Packing::Grouped, Packing::PerBuffer] {
            let err = compress_buffers(&[noise_buffer(1)], packing).unwrap_err();
            assert!(matches!(err, Error::Compression(_)), "{packing:?}: {err:?}");
        }
    }
}
