use std::sync::atomic::{AtomicU32, Ordering};

/// Global sequence counter for dump filenames.
static DUMP_SEQ: AtomicU32 = AtomicU32::new(0);

fn next_dump_path(suffix: &str) -> Option<String> {
    let dir = std::env::var("KATASYMBOL_DUMP_DIR")
        .ok()
        .filter(|d| !d.is_empty())?;
    let seq = DUMP_SEQ.fetch_add(1, Ordering::Relaxed);
    Some(format!("{dir}/katasymbol_{seq:04}{suffix}"))
}

fn write_dump(path: &str, buf: &[u8], label: &str) {
    match std::fs::write(path, buf) {
        Ok(()) => log::info!("{label}: wrote {path} ({} bytes)", buf.len()),
        Err(e) => log::error!("{label}: failed to write {path}: {e}"),
    }
}

/// If `KATASYMBOL_DUMP_DIR` is set, write the raster as PBM P4.
///
/// The raster is already row-major 1bpp MSB-first, which is exactly PBM P4 format.
pub fn dump_pbm(raster: &[u8], width: u32, height: u32, bytes_per_line: u32) {
    let path = match next_dump_path(".pbm") {
        Some(p) => p,
        None => return,
    };

    let header = format!("P4\n{width} {height}\n");
    let data_len = (height * bytes_per_line) as usize;
    let data = if raster.len() >= data_len {
        &raster[..data_len]
    } else {
        raster
    };

    let mut buf = Vec::with_capacity(header.len() + data.len());
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(data);

    write_dump(&path, &buf, "dump_pbm");
}

/// Dump the printhead-sized canvas (column-major LSB-first) as a viewable PBM P4.
///
/// Converts column-major LSB-first data back to row-major MSB-first PBM.
/// The resulting image is `printhead_dots` (384) pixels tall × `num_cols` pixels wide.
pub fn dump_printhead_pbm(canvas: &[u8], num_cols: u32, canvas_bpl: u32, printhead_dots: u32) {
    let path = match next_dump_path("_printhead.pbm") {
        Some(p) => p,
        None => return,
    };

    let out_width = num_cols;
    let out_height = printhead_dots;
    let out_bpl = out_width.div_ceil(8) as usize;

    let header = format!("P4\n{out_width} {out_height}\n");
    let mut buf = Vec::with_capacity(header.len() + out_bpl * out_height as usize);
    buf.extend_from_slice(header.as_bytes());

    for y in 0..out_height {
        let mut row = vec![0u8; out_bpl];
        for x in 0..out_width {
            // Read from column-major LSB-first canvas
            let src_byte = x as usize * canvas_bpl as usize + (y / 8) as usize;
            let src_bit = y % 8; // LSB-first
            if src_byte < canvas.len() && (canvas[src_byte] >> src_bit) & 1 != 0 {
                // Write to row-major MSB-first PBM
                row[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        buf.extend_from_slice(&row);
    }

    write_dump(&path, &buf, "dump_printhead_pbm");
}

/// Accumulator for pre-dither 8bpp PGM dump.
///
/// Created at job start, fed raw 8bpp grayscale lines during rwriteline,
/// flushed as PGM P5 at end-page.
pub struct PgmAccumulator {
    width: u32,
    height: u32,
    data: Vec<u8>,
    lines_received: u32,
}

impl PgmAccumulator {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0u8; (width * height) as usize],
            lines_received: 0,
        }
    }

    /// Append a single 8bpp grayscale scanline.
    pub fn push_line(&mut self, y: u32, line: &[u8]) {
        if y >= self.height {
            return;
        }
        let offset = (y * self.width) as usize;
        let copy_len = line.len().min(self.width as usize);
        self.data[offset..offset + copy_len].copy_from_slice(&line[..copy_len]);
        self.lines_received += 1;
    }

    /// Flush accumulated data as PGM P5 to `$KATASYMBOL_DUMP_DIR/katasymbol_NNNN_pre.pgm`.
    pub fn flush(&self) {
        let path = match next_dump_path("_pre.pgm") {
            Some(p) => p,
            None => return,
        };

        let header = format!("P5\n{} {}\n255\n", self.width, self.height);
        let data_len = (self.height * self.width) as usize;
        let data = if self.data.len() >= data_len {
            &self.data[..data_len]
        } else {
            &self.data
        };

        let mut buf = Vec::with_capacity(header.len() + data.len());
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(data);

        write_dump(&path, &buf, "dump_pgm");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgm_accumulator() {
        let mut acc = PgmAccumulator::new(4, 2);
        acc.push_line(0, &[0xFF, 0x80, 0x40, 0x00]);
        acc.push_line(1, &[0x00, 0x40, 0x80, 0xFF]);
        assert_eq!(acc.lines_received, 2);
        assert_eq!(acc.data[0], 0xFF);
        assert_eq!(acc.data[4], 0x00);
        assert_eq!(acc.data[7], 0xFF);
    }
}
