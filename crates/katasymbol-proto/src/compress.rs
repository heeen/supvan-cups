use crate::error::{Error, Result};

/// Compress data using LZMA1 (alone format) with printer-compatible parameters.
///
/// Parameters: dict_size=8192, lc=3, lp=0, pb=2 (from Android LzmaUtils.java).
/// The printer firmware has limited RAM - larger dictionary sizes will fail.
///
/// Patches the LZMA header to include the exact uncompressed size (Python's
/// lzma module writes -1 by default; we write the real size).
pub fn compress_lzma(data: &[u8]) -> Result<Vec<u8>> {
    use xz2::stream::{LzmaOptions, Stream};

    let mut opts = LzmaOptions::new_preset(6)
        .map_err(|e| Error::Compression(format!("preset: {e}")))?;
    opts.dict_size(8192)
        .literal_context_bits(3)
        .literal_position_bits(0)
        .position_bits(2)
        .nice_len(128);

    let stream = Stream::new_lzma_encoder(&opts)
        .map_err(|e| Error::Compression(format!("encoder: {e}")))?;

    let mut compressed = Vec::with_capacity(data.len());
    let mut encoder = xz2::write::XzEncoder::new_stream(&mut compressed, stream);
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

/// Compress concatenated print buffers for transfer.
///
/// Takes a slice of 4096-byte print buffers, concatenates them, and compresses
/// as a single LZMA stream. Returns (compressed_data, average_compressed_per_buffer).
pub fn compress_buffers(buffers: &[[u8; 4096]]) -> Result<(Vec<u8>, usize)> {
    if buffers.is_empty() {
        return Err(Error::InvalidParam("no buffers to compress".into()));
    }

    let mut concat = Vec::with_capacity(buffers.len() * 4096);
    for buf in buffers {
        concat.extend_from_slice(buf);
    }

    let compressed = compress_lzma(&concat)?;
    let avg = compressed.len() / buffers.len();

    Ok((compressed, avg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_lzma_header() {
        let data = vec![0u8; 4096];
        let compressed = compress_lzma(&data).unwrap();

        // Check header
        assert!(compressed.len() >= 13, "compressed too short: {}", compressed.len());
        // Properties byte: lc=3, lp=0, pb=2 -> 0x5D
        assert_eq!(compressed[0], 0x5D, "wrong properties byte");
        // Dict size: 8192 LE
        assert_eq!(
            &compressed[1..5],
            &8192u32.to_le_bytes(),
            "wrong dict size"
        );
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

        // Decompress using xz2 and verify
        use xz2::stream::Stream;
        let stream = Stream::new_lzma_decoder(u64::MAX).unwrap();
        let mut decompressed = Vec::new();
        let mut decoder = xz2::write::XzDecoder::new_stream(&mut decompressed, stream);
        std::io::Write::write_all(&mut decoder, &compressed).unwrap();
        std::io::Write::flush(&mut decoder).unwrap();
        drop(decoder);
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_buffers() {
        let buf = [0u8; 4096];
        let buffers = vec![buf; 3];
        let (compressed, avg) = compress_buffers(&buffers).unwrap();
        assert!(compressed.len() > 13); // at least header
        assert!(avg > 0);
    }
}
