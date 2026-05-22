use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use serde::Serialize;

/// Global sequence counter — allocated per page via [`JobDump::allocate`].
static DUMP_SEQ: AtomicU32 = AtomicU32::new(0);

/// Resolve the dump directory once per process.
///
/// Priority:
/// 1. Explicit `SUPVAN_DUMP_DIR` env var.
/// 2. In `SUPVAN_MOCK=1` mode, default to `$XDG_RUNTIME_DIR/supvan-mock`
///    (falling back to `/tmp/supvan-mock` if `XDG_RUNTIME_DIR` is unset).
/// 3. Outside mock mode with no explicit env var: `None` (no dumps).
fn dump_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let explicit = std::env::var("SUPVAN_DUMP_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let resolved = if let Some(p) = explicit {
            Some(p)
        } else if crate::util::is_mock_mode() {
            let xdg = std::env::var("XDG_RUNTIME_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/tmp".to_string());
            Some(PathBuf::from(format!("{xdg}/supvan-mock")))
        } else {
            None
        };
        if let Some(ref dir) = resolved {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("dump: cannot create {}: {e}", dir.display());
                return None;
            }
            log::info!("dump: writing to {}", dir.display());
        }
        resolved
    })
    .as_ref()
}

/// True if dump writes will land somewhere — explicit `SUPVAN_DUMP_DIR` or
/// the mock-mode default.
pub fn dumps_enabled() -> bool {
    dump_dir().is_some()
}

fn write_dump(path: &str, buf: &[u8], label: &str) {
    match std::fs::write(path, buf) {
        Ok(()) => log::info!("{label}: wrote {path} ({} bytes)", buf.len()),
        Err(e) => log::error!("{label}: failed to write {path}: {e}"),
    }
}

/// One page's worth of dump artefacts, sharing a sequence number.
///
/// Allocate once per page (at the top of `KsJob::transfer_page`); all writes
/// land as `<dir>/supvan_NNNN.{pbm,printhead.pbm,pre.pgm,manifest.json}`.
pub struct JobDump {
    base: Option<String>,
}

impl JobDump {
    pub fn allocate() -> Self {
        let dir = match dump_dir() {
            Some(d) => d,
            None => return Self { base: None },
        };
        let seq = DUMP_SEQ.fetch_add(1, Ordering::Relaxed);
        Self {
            base: Some(format!("{}/supvan_{seq:04}", dir.display())),
        }
    }

    /// Write the row-major 1bpp raster as PBM P4.
    pub fn pbm(&self, raster: &[u8], width: u32, height: u32, bytes_per_line: u32) {
        let Some(base) = &self.base else { return };
        let path = format!("{base}.pbm");
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

    /// Dump the printhead-sized canvas (column-major LSB-first) as viewable PBM.
    pub fn printhead_pbm(
        &self,
        canvas: &[u8],
        num_cols: u32,
        canvas_bpl: u32,
        printhead_dots: u32,
    ) {
        let Some(base) = &self.base else { return };
        let path = format!("{base}_printhead.pbm");
        let out_width = num_cols;
        let out_height = printhead_dots;
        let out_bpl = out_width.div_ceil(8) as usize;

        let header = format!("P4\n{out_width} {out_height}\n");
        let mut buf = Vec::with_capacity(header.len() + out_bpl * out_height as usize);
        buf.extend_from_slice(header.as_bytes());
        for y in 0..out_height {
            let mut row = vec![0u8; out_bpl];
            for x in 0..out_width {
                let src_byte = x as usize * canvas_bpl as usize + (y / 8) as usize;
                let src_bit = y % 8;
                if src_byte < canvas.len() && (canvas[src_byte] >> src_bit) & 1 != 0 {
                    row[(x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
            buf.extend_from_slice(&row);
        }
        write_dump(&path, &buf, "dump_printhead_pbm");
    }

    /// Flush a pre-dither PGM accumulator under this page's name.
    pub fn pgm(&self, acc: &PgmAccumulator) {
        let Some(base) = &self.base else { return };
        let path = format!("{base}_pre.pgm");
        acc.write_to(&path);
    }

    /// Write the job manifest as JSON next to the PBMs.
    pub fn manifest(&self, manifest: &JobManifest) {
        let Some(base) = &self.base else { return };
        let path = format!("{base}_manifest.json");
        match serde_json::to_vec_pretty(manifest) {
            Ok(buf) => write_dump(&path, &buf, "dump_manifest"),
            Err(e) => log::error!("dump_manifest: serialise failed: {e}"),
        }
    }
}

/// Metadata recorded alongside the per-page raster dumps.
#[derive(Debug, Clone, Serialize)]
pub struct JobManifest {
    pub timestamp: String,
    pub printer_name: String,
    pub width: u32,
    pub height: u32,
    pub bytes_per_line: u32,
    pub density: u8,
    pub printhead_width_dots: u32,
    pub copies: u32,
    pub mock: bool,
    pub simulated_outcome: String,
    pub elapsed_ms: u128,
}

/// Accumulator for pre-dither 8bpp PGM dump.
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

    pub fn push_line(&mut self, y: u32, line: &[u8]) {
        if y >= self.height {
            return;
        }
        let offset = (y * self.width) as usize;
        let copy_len = line.len().min(self.width as usize);
        self.data[offset..offset + copy_len].copy_from_slice(&line[..copy_len]);
        self.lines_received += 1;
    }

    fn write_to(&self, path: &str) {
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
        write_dump(path, &buf, "dump_pgm");
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
