use crate::error::{Error, Result};

/// Largest compressed block the firmware will accept in one transfer — the
/// size of its receive buffer.
const MAX_COMPRESSED_BLOCK: usize = crate::buffer::PRINT_BUF_SIZE;

/// Most print buffers the vendor packs into one compressed block
/// (`T50PlusPrint.bufferMAXCount`).
const BUFFER_MAX_COUNT: usize = 4;

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
/// until the result fits the firmware's receive buffer:
///
/// ```java
/// int min = Math.min(this.bufferMAXCount, list.size());
/// do {
///     for (int i = 0; i < min; i++) byteArrayOutputStream.write(list.get(i));
///     bArr = LzmaUtils.LzmaEncode(byteArrayOutputStream.toByteArray());
///     min--;
/// } while (bArr.length > 4096);
/// ```
///
/// (`T80ProPrint.multiCompression`; `T50PlusPrint` carries the same
/// `bufferMAXCount = 4` but JADX could not decompile its copy.)
///
/// Both bounds matter. One stream for a whole page overruns the receive buffer
/// — an 80mm label compresses to 4533 bytes — and prints garbled. One block
/// per buffer is equally wrong: eight transfers for the same page leaves the
/// printer marking a few millimetres and stopping.
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
            take -= 1;
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

/// Compress profile-sized buffers one-at-a-time.
///
/// E-series printers expect exactly one LZMA stream per print buffer.
pub fn compress_buffers_individually(
    buffers: &[Vec<u8>],
) -> Result<(Vec<Vec<u8>>, usize)> {
    if buffers.is_empty() {
        return Err(Error::InvalidParam("no buffers to compress".into()));
    }

    let mut blocks = Vec::with_capacity(buffers.len());

    for buffer in buffers {
        blocks.push(compress_lzma(buffer)?);
    }

    let avg = blocks.iter().map(Vec::len).sum::<usize>() / buffers.len();

    log::debug!(
        "compress individually: {} buffers -> {} block(s) {:?}",
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

    /// Compressible buffers pack up to the vendor's cap, not all-in-one: a
    /// whole page in a single stream overruns the firmware's receive buffer.
    #[test]
    fn packing_is_capped_at_the_vendor_group_size() {
        let buffers = vec![[0u8; 4096]; 9];
        let (blocks, avg) = compress_buffers(&buffers).unwrap();
        assert_eq!(blocks.len(), 3, "9 buffers pack 4 + 4 + 1");
        assert!(avg > 0);
        let round: Vec<u8> = blocks
            .iter()
            .flat_map(|b| decompress_lzma(b).unwrap())
            .collect();
        assert_eq!(round, buffers.concat(), "buffers lost or reordered");
    }

    /// A group too big compressed shrinks until it fits, rather than being
    /// sent over the receive-buffer size.
    #[test]
    fn e_series_buffers_compress_individually() {
        let buffers = vec![
            vec![0x11u8; 4000],
            vec![0x22u8; 4000],
        ];

        let (blocks, avg) = compress_buffers_individually(&buffers).unwrap();

        assert_eq!(blocks.len(), 2);
        assert!(avg > 0);

        assert_eq!(decompress_lzma(&blocks[0]).unwrap(), buffers[0]);
        assert_eq!(decompress_lzma(&blocks[1]).unwrap(), buffers[1]);
    }

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
