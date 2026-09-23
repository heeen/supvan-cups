use crate::error::{Error, Result};

/// Largest compressed block the firmware will accept in one transfer — the
/// size of its receive buffer.
const MAX_COMPRESSED_BLOCK: usize = crate::buffer::PRINT_BUF_SIZE;

/// Most print buffers the vendor packs into one compressed block.
///
/// This was 4, taken from `T80ProPrint.bufferMAXCount` in the JADX
/// decompilation. That is the T80 Pro's value, and `T50PlusPrint` was assumed
/// to share it because its `multiCompression()` would not decompile. The T50
/// family's own encoder is readable in the desktop (Electron) app — webpack
/// module `45ac`, the one `isT5080(getDevType(ProductId))` dispatches to — and
/// it packs `const b = 32`.
///
/// `isT5080` covers the T50 *and* T80 families, so both share this encoder. On
/// a T50 Pro a 50×80 mm page is 8 buffers: at 32 it packs into a single block
/// and prints in full; at 4 it splits in two and the printer lays down only the
/// first block's worth.
const BUFFER_MAX_COUNT: usize = 32;

/// Compress data using LZMA1 (alone format) with printer-compatible parameters.
///
/// Parameters: dict_size=8192, lc=3, lp=0, pb=2 (from Android LzmaUtils.java).
/// The printer firmware has limited RAM - larger dictionary sizes will fail.
///
/// Patches the LZMA header to include the exact uncompressed size (Python's
/// lzma module writes -1 by default; we write the real size).
pub fn compress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    use liblzma::stream::{LzmaOptions, Stream};

    let mut opts =
        LzmaOptions::new_preset(6).map_err(|e| Error::Compression(format!("preset: {e}")))?;
    opts.dict_size(8192)
        .literal_context_bits(3)
        .literal_position_bits(0)
        .position_bits(2)
        .nice_len(128);

    let stream =
        Stream::new_lzma_encoder(&opts).map_err(|e| Error::Compression(format!("encoder: {e}")))?;

    let mut compressed = Vec::with_capacity(data.len());
    let mut encoder = liblzma::write::XzEncoder::new_stream(&mut compressed, stream);
    std::io::Write::write_all(&mut encoder, data)
        .map_err(|e| Error::Compression(format!("write: {e}")))?;
    encoder
        .finish()
        .map_err(|e| Error::Compression(format!("finish: {e}")))?;

    // The XzEncoder with LZMA encoder stream produces raw LZMA1 alone format:
    //   [0]     properties byte (lc + lp*9 + pb*45 = 3 + 0 + 90 = 93 = 0x5D)
    //   [1..4]  dict_size LE (8192 = 0x00002000)
    //   [5..12] uncompressed size LE (or 0xFFFFFFFFFFFFFFFF for unknown)
    //   [13..]  compressed data

    // Patch header to ensure correct uncompressed size
    if compressed.len() >= 13 {
        let size_bytes = (data.len() as u64).to_le_bytes();
        compressed[5..13].copy_from_slice(&size_bytes);
    }

    Ok(compressed)
}

/// Decompress an LZMA1-alone stream produced by [`compress_lzma`].
///
/// `compress_lzma` patches the alone header with the definite uncompressed size
/// (what the printer firmware reads); combined with the encoder's trailing
/// end-of-stream marker, strict liblzma builds (e.g. the bundled liblzma CI
/// links, reproducible with `LZMA_API_STATIC=1`) reject that as
/// `LZMA_DATA_ERROR`. This restores the "unknown size" sentinel so liblzma
/// decodes the encoder's native marker-terminated stream.
pub fn decompress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    use liblzma::stream::Stream;
    use std::io::Read;

    if data.len() < 13 {
        return Err(Error::Compression(format!(
            "lzma stream too short: {} bytes",
            data.len()
        )));
    }
    let mut stream_bytes = data.to_vec();
    stream_bytes[5..13].copy_from_slice(&u64::MAX.to_le_bytes());
    let stream = Stream::new_lzma_decoder(u64::MAX)
        .map_err(|e| Error::Compression(format!("decoder: {e}")))?;
    let mut decoder = liblzma::read::XzDecoder::new_stream(stream_bytes.as_slice(), stream);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Compression(format!("decompress: {e}")))?;
    Ok(out)
}

/// Split print buffers into the LZMA blocks the firmware expects, one block
/// per transfer.
///
/// Pack up to [`BUFFER_MAX_COUNT`] buffers, compress, and shrink the group
/// until the result fits [`MAX_COMPRESSED_BLOCK`]. The T50/T80 encoder's loop
/// (desktop SupvanEditor 1.1.7, `js/app.*.js`, webpack module `45ac`):
///
/// ```js
/// const T = 4e3, b = 32;
/// while (v < o.length) {
///     let e = Math.min(b, o.length - v);
///     while (e >= 1) {
///         n = getLzma(concat(o[v .. v + e]));
///         if (n.length <= T) break;
///         e = Math.floor(0.75 * e);
///         if (e < 1) e = 1;
///     }
///     sendData.push(n);
///     v += e;
/// }
/// ```
///
/// The group cap is what makes or breaks a long label. A 50×80 mm page is 8
/// buffers: at 32 they pack into **one** block and the whole label prints; at
/// 4 they split in two, and the printer lays down only the first block's
/// worth — accepted, acknowledged, silently truncated, no error flag, and CUPS
/// still reports the job complete.
///
/// The compressed cap keeps a block inside the firmware's receive buffer. One
/// stream for a page that does not fit overruns it and prints garbled. The
/// vendor's own `T` is 4000, just under that buffer; we hold at the buffer size
/// itself because this function serves every family, and the other encoders the
/// vendor dispatches to are built around a 4096 buffer.
///
/// Note that the vendor packs per family, and this is the T5080 rule only. It is
/// the correct rule for `supvan_t50` and `supvan_t80`, which share that encoder;
/// the G, TP and SP families are not covered by it.
///
/// Returns the blocks and the mean compressed size per buffer, which feeds
/// `calc_speed`.
pub fn compress_buffers(
    buffers: &[[u8; crate::buffer::PRINT_BUF_SIZE]],
) -> Result<(Vec<Vec<u8>>, usize)> {
    if buffers.is_empty() {
        return Err(Error::InvalidParam("no buffers to compress".into()));
    }

    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut next = 0usize;
    while next < buffers.len() {
        let mut take = BUFFER_MAX_COUNT.min(buffers.len() - next);
        let block = loop {
            let group: Vec<u8> = buffers[next..next + take].concat();
            let z = compress_lzma(&group)?;
            if z.len() <= MAX_COMPRESSED_BLOCK {
                break z;
            }
            if take == 1 {
                // No smaller group to fall back to. Send it and say so —
                // raster this incompressible is not what the firmware is
                // sized for.
                log::warn!(
                    "print buffer compresses to {} bytes, past the \
                     {MAX_COMPRESSED_BLOCK}-byte receive buffer; sending anyway",
                    z.len()
                );
                break z;
            }
            // The vendor shrinks by a quarter per attempt
            // (`e = Math.floor(0.75 * e)`) rather than one buffer at a time;
            // on a page that does not fit, that is the difference between two
            // or three LZMA passes and a dozen.
            take = (take * 3 / 4).max(1);
        };
        blocks.push(block);
        next += take;
    }

    let avg = blocks.iter().map(Vec::len).sum::<usize>() / buffers.len();
    log::debug!(
        "compress: {} buffers -> {} block(s) {:?}",
        buffers.len(),
        blocks.len(),
        blocks.iter().map(Vec::len).collect::<Vec<_>>()
    );
    Ok((blocks, avg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_lzma_header() {
        let data = vec![0u8; 4096];
        let compressed = compress_lzma(&data).unwrap();

        // Check header
        assert!(
            compressed.len() >= 13,
            "compressed too short: {}",
            compressed.len()
        );
        // Properties byte: lc=3, lp=0, pb=2 -> 0x5D
        assert_eq!(compressed[0], 0x5D, "wrong properties byte");
        // Dict size: 8192 LE
        assert_eq!(&compressed[1..5], &8192u32.to_le_bytes(), "wrong dict size");
        // Uncompressed size: 4096 LE
        assert_eq!(
            &compressed[5..13],
            &4096u64.to_le_bytes(),
            "wrong uncompressed size"
        );
    }

    #[test]
    fn test_compress_roundtrip() {
        let data = vec![0x42u8; 1024];
        let compressed = compress_lzma(&data).unwrap();

        // Verifies the LZMA payload round-trips via the shared decoder. The
        // patched header bytes are checked separately by `test_compress_lzma_header`.
        let decompressed = decompress_lzma(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    /// A short page packs into a single block, which is what makes an 8-buffer
    /// 50×80 mm label print in full. Splitting it in two is the bug that left
    /// half a label on the platen.
    #[test]
    fn a_long_page_packs_into_one_block() {
        // 8 buffers is a 50×80 mm page on a T50 Pro.
        let buffers = vec![[0u8; 4096]; 8];
        let (blocks, avg) = compress_buffers(&buffers).unwrap();
        assert_eq!(blocks.len(), 1, "8 buffers must travel as one block");
        assert!(avg > 0);
        let round: Vec<u8> = blocks
            .iter()
            .flat_map(|b| decompress_lzma(b).unwrap())
            .collect();
        assert_eq!(round, buffers.concat(), "buffers lost or reordered");
    }

    /// The group cap still bounds a page: past [`BUFFER_MAX_COUNT`] buffers the
    /// page has to take more than one block.
    #[test]
    fn groups_past_the_cap_split_into_blocks() {
        // All-zero buffers compress far below the cap, so the split is driven
        // by the group size alone.
        let buffers = vec![[0u8; 4096]; 64];
        let (blocks, _) = compress_buffers(&buffers).unwrap();
        assert_eq!(blocks.len(), 2, "64 buffers pack 32 + 32");
        let round: Vec<u8> = blocks
            .iter()
            .flat_map(|b| decompress_lzma(b).unwrap())
            .collect();
        assert_eq!(round, buffers.concat(), "buffers lost or reordered");
    }

    /// A group too big compressed shrinks until it fits, rather than being
    /// sent over the receive-buffer size.
    #[test]
    fn oversized_groups_shrink_to_fit() {
        // Pseudo-random bytes: LZMA cannot shrink these, so four together
        // would compress to roughly 16KB and must be split.
        let mut seed = 0x12345678u32;
        let buffers: Vec<[u8; 4096]> = (0..4)
            .map(|_| {
                let mut b = [0u8; 4096];
                for byte in b.iter_mut() {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    *byte = (seed >> 24) as u8;
                }
                b
            })
            .collect();

        let (blocks, _) = compress_buffers(&buffers).unwrap();
        assert_eq!(blocks.len(), 4, "incompressible data cannot be grouped");
        let round: Vec<u8> = blocks
            .iter()
            .flat_map(|b| decompress_lzma(b).unwrap())
            .collect();
        assert_eq!(round, buffers.concat());
    }
}
