/// Thermal-compensated sRGB-to-dither LUT.
///
/// Combines standard sRGB linearization (gamma ~2.2) with a thermal bleed
/// compensation curve (gamma correction factor = 5.0). This pushes mid-tones
/// significantly lighter to compensate for dot spread on thermal printers,
/// where anything above ~50% dot density appears solid black.
///
/// Key mappings (W colorspace: 0=black, 255=white):
///   W=  0 -> 100% dots, W= 48 -> 50%, W=128 -> 25%, W=192 -> 12.5%, W=255 -> 0%
pub static SRGB_TO_LINEAR: [u8; 256] = [
    0, 50, 58, 63, 67, 70, 72, 74, 76, 78, 80, 82, 83, 85, 86, 88, 89, 90, 92, 93, 95, 96, 97, 98,
    100, 101, 102, 103, 105, 106, 107, 108, 109, 110, 112, 113, 114, 115, 116, 117, 118, 119, 120,
    121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 135, 136, 137, 138,
    139, 140, 141, 142, 142, 143, 144, 145, 146, 147, 148, 148, 149, 150, 151, 152, 152, 153, 154,
    155, 156, 156, 157, 158, 159, 159, 160, 161, 162, 162, 163, 164, 165, 165, 166, 167, 167, 168,
    169, 170, 170, 171, 172, 172, 173, 174, 174, 175, 176, 177, 177, 178, 179, 179, 180, 181, 181,
    182, 183, 183, 184, 184, 185, 186, 186, 187, 188, 188, 189, 190, 190, 191, 191, 192, 193, 193,
    194, 195, 195, 196, 196, 197, 198, 198, 199, 199, 200, 201, 201, 202, 202, 203, 203, 204, 205,
    205, 206, 206, 207, 207, 208, 209, 209, 210, 210, 211, 211, 212, 213, 213, 214, 214, 215, 215,
    216, 216, 217, 217, 218, 219, 219, 220, 220, 221, 221, 222, 222, 223, 223, 224, 224, 225, 225,
    226, 226, 227, 227, 228, 228, 229, 230, 230, 231, 231, 232, 232, 233, 233, 234, 234, 235, 235,
    236, 236, 237, 237, 238, 238, 238, 239, 239, 240, 240, 241, 241, 242, 242, 243, 243, 244, 244,
    245, 245, 246, 246, 247, 247, 248, 248, 249, 249, 249, 250, 250, 251, 251, 252, 252, 253, 253,
    254, 254, 255, 255,
];

/// 4x4 Bayer ordered dither threshold matrix, scaled 0-255.
pub static BAYER4: [[u8; 4]; 4] = [
    [8, 136, 40, 168],
    [200, 72, 232, 104],
    [56, 184, 24, 152],
    [248, 120, 216, 88],
];

/// Dither an 8bpp sRGB grayscale line to 1bpp MSB-first, with horizontal mirror.
///
/// `line`: input grayscale pixels (0x00 = black, 0xFF = white / W colorspace), length >= `width`.
/// `width`: number of pixels.
/// `y`: current scanline index (for Bayer pattern row selection).
/// `mono`: output 1bpp buffer, length >= `(width + 7) / 8`. Caller must zero it first.
pub fn dither_line(line: &[u8], width: u32, y: u32, mono: &mut [u8]) {
    let bayer_row = &BAYER4[(y & 3) as usize];
    for x in 0..width {
        let mx = width - 1 - x; // mirror
        let linear = SRGB_TO_LINEAR[line[x as usize] as usize];
        if linear < bayer_row[(mx & 3) as usize] {
            mono[(mx / 8) as usize] |= 0x80 >> (mx & 7);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dither_all_black() {
        let line = vec![0x00; 8]; // all black (W colorspace)
        let mut mono = vec![0u8; 1];
        dither_line(&line, 8, 0, &mut mono);
        // All pixels are 0 -> LUT[0]=0 < any threshold -> all bits set
        assert_eq!(mono[0], 0xFF);
    }

    #[test]
    fn test_dither_all_white() {
        let line = vec![0xFF; 8]; // all white (W colorspace)
        let mut mono = vec![0u8; 1];
        dither_line(&line, 8, 0, &mut mono);
        // All pixels are 255 -> LUT[255]=255 < threshold never -> all bits clear
        assert_eq!(mono[0], 0x00);
    }

    #[test]
    fn test_dither_midtone_lighter_than_50_percent() {
        // With thermal compensation, sRGB mid-gray should produce well under 50% dots
        let line = vec![0x80; 32]; // sRGB 128 → LUT value 188 → ~25% dots
        let mut total_bits = 0u32;
        for y in 0..4 {
            let mut mono = vec![0u8; 4];
            dither_line(&line, 32, y, &mut mono);
            for &b in &mono {
                total_bits += b.count_ones();
            }
        }
        // With gamma=5 compensation, sRGB 0x80 → ~25% coverage (32 of 128)
        assert!(
            total_bits > 16 && total_bits < 64,
            "expected ~25% bits set, got {total_bits}/128"
        );
    }

    #[test]
    fn test_dither_output_size() {
        let line = vec![0x80; 13]; // non-aligned width
        let bpl = 13_usize.div_ceil(8);
        let mut mono = vec![0u8; bpl];
        dither_line(&line, 13, 0, &mut mono);
        // Just verify it doesn't panic
    }
}
