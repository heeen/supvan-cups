use crate::buffer::{PRINT_BUF_SIZE, printed_cols};
use crate::error::{Error, Result};
use crate::speed::{MAX_SPEED, SPLIT_PAGE_SPEED, calc_speed};
use liblzma_sys as sys;

/// Largest compressed block the firmware will accept in one transfer — the
/// size of its receive buffer.
const MAX_COMPRESSED_BLOCK: usize = PRINT_BUF_SIZE;

/// Maximum print buffers per LZMA block on the Android/SPP path —
/// `bufferMAXCount` in `T50PlusPrint.multiCompression()`.
const MAX_GROUP_BUFFERS: usize = 4;

/// Electron's cut-over (`getEncodeData`): when the buffers, compressed one at
/// a time, sum past this, every buffer goes out as its own block.
const PER_BUFFER_THRESHOLD: usize = 4000;

/// LZMA1 parameters both vendor encoders use (Android `LzmaUtils`).
const PRESET: u32 = 6;
const DICT_SIZE: u32 = 8192;
const LC: u32 = 3;
const LP: u32 = 0;
const PB: u32 = 2;
const NICE_LEN: u32 = 128;

/// The `.lzma` ("alone") header: props byte, dict size (u32), uncompressed
/// size (u64).
const ALONE_HEADER_LEN: usize = 13;

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
    pub source_buffers: usize,
    /// Feed columns those buffers print — the length of label this block covers.
    pub printed_cols: u32,
}

impl CompressedBlock {
    fn new(data: Vec<u8>, buffers: &[[u8; PRINT_BUF_SIZE]]) -> Self {
        Self {
            data,
            source_buffers: buffers.len(),
            printed_cols: buffers.iter().map(printed_cols).sum(),
        }
    }
}

/// A page's blocks plus the one speed they all go out at (BUF_FULL's second
/// parameter). Changing feed speed mid-label leaves a visible seam.
#[derive(Debug, Clone)]
pub struct CompressedPage {
    pub blocks: Vec<CompressedBlock>,
    pub speed: u16,
}

/// LZMA1EXT options with the vendor parameters and no end marker.
fn lzma1ext_options() -> Result<sys::lzma_options_lzma> {
    // SAFETY: a plain C struct for which all-zero is a valid initial state.
    let mut opts: sys::lzma_options_lzma = unsafe { std::mem::zeroed() };
    // SAFETY: `opts` is a valid, writable options struct.
    if unsafe { sys::lzma_lzma_preset(&mut opts, PRESET) } != 0 {
        return Err(Error::Compression(format!(
            "lzma preset {PRESET} unsupported"
        )));
    }
    opts.dict_size = DICT_SIZE;
    opts.lc = LC;
    opts.lp = LP;
    opts.pb = PB;
    opts.nice_len = NICE_LEN;
    // Without LZMA_LZMA1EXT_ALLOW_EOPM the encoder writes no end marker.
    opts.ext_flags = 0;
    Ok(opts)
}

/// A one-filter chain over `opts`. The pointer inside borrows `opts`, which
/// has to outlive any coder built from the chain.
fn lzma1ext_filters(opts: &mut sys::lzma_options_lzma) -> [sys::lzma_filter; 2] {
    [
        sys::lzma_filter {
            id: LZMA_FILTER_LZMA1EXT,
            options: (opts as *mut sys::lzma_options_lzma).cast(),
        },
        sys::lzma_filter {
            id: sys::LZMA_VLI_UNKNOWN,
            options: std::ptr::null_mut(),
        },
    ]
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
pub(crate) fn compress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    let mut opts = lzma1ext_options()?;
    let filters = lzma1ext_filters(&mut opts);

    let mut out = Vec::with_capacity(ALONE_HEADER_LEN + data.len());
    out.push((PB * 5 + LP) as u8 * 9 + LC as u8);
    out.extend_from_slice(&DICT_SIZE.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());

    // SAFETY: LZMA_STREAM_INIT is all zeros.
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
            out.reserve(PRINT_BUF_SIZE);
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
) -> Result<CompressedPage> {
    if buffers.is_empty() {
        return Err(Error::InvalidParam("no buffers to compress".into()));
    }
    let page = match packing {
        Packing::Grouped => compress_grouped(buffers)?,
        Packing::PerBuffer => compress_per_buffer(buffers)?,
    };
    log::debug!(
        "compress ({packing:?}): {} buffers -> {:?} at speed {}",
        buffers.len(),
        page.blocks
            .iter()
            .map(|b| (b.data.len(), b.source_buffers))
            .collect::<Vec<_>>(),
        page.speed
    );
    Ok(page)
}

/// Android `multiCompression`. Its speed is a running minimum computed while
/// the page is still being encoded, so early blocks can go out faster than
/// later ones; the page minimum here is what it converges to.
fn compress_grouped(buffers: &[[u8; PRINT_BUF_SIZE]]) -> Result<CompressedPage> {
    let mut blocks = Vec::new();
    let mut speed = MAX_SPEED;
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
        speed = speed.min(calc_speed(data.len() / take));
        let (group, tail) = rest.split_at(take);
        blocks.push(CompressedBlock::new(data, group));
        rest = tail;
    }
    Ok(CompressedPage { blocks, speed })
}

/// Electron `getEncodeData`.
fn compress_per_buffer(buffers: &[[u8; PRINT_BUF_SIZE]]) -> Result<CompressedPage> {
    let singles = buffers
        .iter()
        .map(|b| compress_lzma(b))
        .collect::<Result<Vec<_>>>()?;
    if singles.iter().map(Vec::len).sum::<usize>() <= PER_BUFFER_THRESHOLD {
        let whole = compress_lzma(&buffers.concat())?;
        if whole.len() <= MAX_COMPRESSED_BLOCK {
            return Ok(CompressedPage {
                blocks: vec![CompressedBlock::new(whole, buffers)],
                speed: MAX_SPEED,
            });
        }
    }
    let speed = if singles.len() > 1 {
        SPLIT_PAGE_SPEED
    } else {
        MAX_SPEED
    };
    let blocks = singles
        .into_iter()
        .zip(buffers.chunks(1))
        .map(|(data, buffer)| {
            if data.len() > MAX_COMPRESSED_BLOCK {
                return Err(oversized(data.len()));
            }
            Ok(CompressedBlock::new(data, buffer))
        })
        .collect::<Result<_>>()?;
    Ok(CompressedPage { blocks, speed })
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

    /// A deterministic byte stream for test data (Numerical Recipes LCG).
    fn lcg(seed: u32) -> impl Iterator<Item = u8> {
        std::iter::successors(Some(seed), |s| {
            Some(s.wrapping_mul(1664525).wrapping_add(1013904223))
        })
        .skip(1)
        .map(|s| (s >> 24) as u8)
    }

    fn noise_buffer(seed: u32) -> [u8; PRINT_BUF_SIZE] {
        let mut b = [0u8; PRINT_BUF_SIZE];
        b.iter_mut().zip(lcg(seed)).for_each(|(d, s)| *d = s);
        b
    }

    /// An 80 mm label: 639 feed columns, eight print buffers. `noisy` makes
    /// every byte depend on its column so the page cannot fit one block.
    fn eighty_mm_buffers(noisy: bool) -> Vec<[u8; PRINT_BUF_SIZE]> {
        let cols: u16 = 639;
        let bpl: u8 = 48;
        let len = cols as usize * bpl as usize;
        let canvas: Vec<u8> = if noisy {
            // Sparse noise: compressible, but not into one block.
            lcg(0x9e37_79b9)
                .zip(lcg(0x7f4a_7c15))
                .map(|(a, b)| a & b & 0x5a)
                .take(len)
                .collect()
        } else {
            (0..len).map(|i| ((i / bpl as usize) % 251) as u8).collect()
        };
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

    fn roundtrip(page: &CompressedPage) -> Vec<u8> {
        page.blocks
            .iter()
            .flat_map(|b| {
                let raw = decompress_lzma(&b.data).unwrap();
                assert_eq!(raw.len(), b.source_buffers * PRINT_BUF_SIZE);
                raw
            })
            .collect()
    }

    #[test]
    fn header_carries_props_dict_and_exact_size() {
        let z = compress_lzma(&[0u8; PRINT_BUF_SIZE]).unwrap();
        assert_eq!(z[0], 0x5D, "lc=3 lp=0 pb=2");
        assert_eq!(&z[1..5], &DICT_SIZE.to_le_bytes());
        assert_eq!(
            &z[5..ALONE_HEADER_LEN],
            &(PRINT_BUF_SIZE as u64).to_le_bytes()
        );
    }

    /// The firmware wants what the vendor encoders produce: no end marker.
    /// A strict LZMA1EXT decoder, told the size and *not* allowed a marker,
    /// rejects a stream that has one.
    #[test]
    fn stream_has_no_end_marker() {
        let data: Vec<u8> = (0..3 * PRINT_BUF_SIZE).map(|i| (i % 7) as u8).collect();
        let z = compress_lzma(&data).unwrap();
        let payload = &z[ALONE_HEADER_LEN..];

        let mut opts = lzma1ext_options().unwrap();
        opts.ext_size_low = data.len() as u32;
        opts.ext_size_high = 0;
        let filters = lzma1ext_filters(&mut opts);
        let mut out = vec![0u8; data.len() + 64];
        // SAFETY: zeroed stream; terminated filter chain; buffers sized below.
        unsafe {
            let mut strm: sys::lzma_stream = std::mem::zeroed();
            assert_eq!(
                sys::lzma_raw_decoder(&mut strm, filters.as_ptr()),
                sys::LZMA_OK
            );
            strm.next_in = payload.as_ptr();
            strm.avail_in = payload.len();
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
        let page = compress_buffers(&buffers, Packing::Grouped).unwrap();
        let sizes: Vec<usize> = page.blocks.iter().map(|b| b.source_buffers).collect();
        assert_eq!(sizes, [4, 4, 1]);
        assert_eq!(roundtrip(&page), buffers.concat());
    }

    #[test]
    fn grouped_page_runs_at_its_slowest_block_speed() {
        // Dense first group, empty tail: the page takes the dense speed.
        let mut buffers: Vec<_> = (0..4).map(noise_buffer).collect();
        for b in &mut buffers {
            // Halve the entropy so a single buffer still fits a block.
            b.iter_mut().step_by(2).for_each(|byte| *byte = 0);
        }
        buffers.push([0u8; PRINT_BUF_SIZE]);
        let page = compress_buffers(&buffers, Packing::Grouped).unwrap();
        let slowest = page
            .blocks
            .iter()
            .map(|b| calc_speed(b.data.len() / b.source_buffers))
            .min()
            .unwrap();
        assert_eq!(page.speed, slowest);
        assert!(slowest < MAX_SPEED);
    }

    #[test]
    fn per_buffer_keeps_a_small_page_in_one_block_at_full_speed() {
        let buffers = eighty_mm_buffers(false);
        let page = compress_buffers(&buffers, Packing::PerBuffer).unwrap();
        assert_eq!(page.blocks.len(), 1);
        assert_eq!(page.blocks[0].source_buffers, 8);
        assert_eq!(page.speed, MAX_SPEED);
        assert_eq!(roundtrip(&page), buffers.concat());
    }

    #[test]
    fn per_buffer_splits_a_dense_page_one_buffer_per_block() {
        let buffers = eighty_mm_buffers(true);
        let page = compress_buffers(&buffers, Packing::PerBuffer).unwrap();
        assert_eq!(page.blocks.len(), 8);
        assert!(page.blocks.iter().all(|b| b.source_buffers == 1));
        assert_eq!(page.speed, SPLIT_PAGE_SPEED);
        assert_eq!(roundtrip(&page), buffers.concat());
    }

    #[test]
    fn incompressible_buffer_is_refused_by_both() {
        for packing in [Packing::Grouped, Packing::PerBuffer] {
            let err = compress_buffers(&[noise_buffer(1)], packing).unwrap_err();
            assert!(matches!(err, Error::Compression(_)), "{packing:?}: {err:?}");
        }
    }
}
