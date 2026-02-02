//! Minimal FFI bindings to libcupsimage for reading CUPS raster data.

use std::os::raw::{c_int, c_uint, c_void};

/// CUPS raster open mode: read from fd.
pub const CUPS_RASTER_READ: c_uint = 0;

/// CUPS color space for black (1bpp monochrome).
pub const _CUPS_CSPACE_K: c_uint = 3;

/// CupsPageHeader2 - 1796 bytes.
/// We only need a few fields; the rest is padding.
/// Field offsets from cups/raster.h (CUPS 2.x):
///   offset 0:   MediaClass[64]
///   offset 64:  MediaColor[64]
///   offset 128: MediaType[64]
///   offset 192: OutputType[64]
///   offset 256: AdvanceDistance (unsigned)
///   offset 260: AdvanceMedia (cups_adv_t)
///   offset 264: Collate (cups_bool_t)
///   offset 268: CutMedia (cups_cut_t)
///   offset 272: Duplex (cups_bool_t)
///   offset 276: HWResolution[2] (unsigned)
///   offset 284: ImagingBoundingBox[4] (unsigned)
///   offset 300: InsertSheet (cups_bool_t)
///   offset 304: Jog (cups_jog_t)
///   offset 308: LeadingEdge (cups_edge_t)
///   offset 312: Margins[2] (unsigned)
///   offset 320: ManualFeed (cups_bool_t)
///   offset 324: MediaPosition (unsigned)
///   offset 328: MediaWeight (unsigned)
///   offset 332: MirrorPrint (cups_bool_t)
///   offset 336: NegativePrint (cups_bool_t)
///   offset 340: NumCopies (unsigned)
///   offset 344: Orientation (cups_orient_t)
///   offset 348: OutputFaceUp (cups_bool_t)
///   offset 352: PageSize[2] (unsigned)
///   offset 360: Separations (cups_bool_t)
///   offset 364: TraySwitch (cups_bool_t)
///   offset 368: Tumble (cups_bool_t)
///   offset 372: cupsWidth (unsigned)
///   offset 376: cupsHeight (unsigned)
///   offset 380: cupsMediaType (unsigned)
///   offset 384: cupsBitsPerColor (unsigned)
///   offset 388: cupsBitsPerPixel (unsigned)
///   offset 392: cupsBytesPerLine (unsigned)
///   offset 396: cupsColorOrder (cups_order_t)
///   offset 400: cupsColorSpace (cups_cspace_t)
///   ...remaining fields up to 1796 bytes
#[repr(C)]
pub struct CupsPageHeader2 {
    pub _pad0: [u8; 276],        // offset 0..275
    pub hw_resolution: [c_uint; 2], // offset 276 (HWResolution)
    pub _pad1: [u8; 16],         // offset 284..299 (ImagingBoundingBox)
    pub _pad2: [u8; 52],         // offset 300..351
    pub page_size: [c_uint; 2],  // offset 352 (PageSize)
    pub _pad3: [u8; 4],          // offset 360..363 (Separations)
    pub _pad4: [u8; 8],          // offset 364..371
    pub cups_width: c_uint,      // offset 372
    pub cups_height: c_uint,     // offset 376
    pub _pad5: c_uint,           // offset 380 (cupsMediaType)
    pub cups_bits_per_color: c_uint, // offset 384
    pub cups_bits_per_pixel: c_uint, // offset 388
    pub cups_bytes_per_line: c_uint, // offset 392
    pub _pad6: c_uint,           // offset 396 (cupsColorOrder)
    pub cups_color_space: c_uint, // offset 400
    pub _pad_rest: [u8; 1392],   // offset 404..1795 (remaining to fill 1796)
}

const _: () = assert!(std::mem::size_of::<CupsPageHeader2>() == 1796);

impl Default for CupsPageHeader2 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

extern "C" {
    pub fn cupsRasterOpen(fd: c_int, mode: c_uint) -> *mut c_void;
    pub fn cupsRasterReadHeader2(r: *mut c_void, h: *mut CupsPageHeader2) -> c_uint;
    pub fn cupsRasterReadPixels(r: *mut c_void, p: *mut u8, len: c_uint) -> c_uint;
    pub fn cupsRasterClose(r: *mut c_void);
}

/// Safe wrapper around CUPS raster reader.
pub struct RasterReader {
    handle: *mut c_void,
}

impl RasterReader {
    /// Open raster reader on the given file descriptor.
    pub fn open(fd: c_int) -> Option<Self> {
        let handle = unsafe { cupsRasterOpen(fd, CUPS_RASTER_READ) };
        if handle.is_null() {
            None
        } else {
            Some(Self { handle })
        }
    }

    /// Read the next page header. Returns None at end of stream.
    pub fn read_header(&mut self) -> Option<CupsPageHeader2> {
        let mut header = CupsPageHeader2::default();
        let ret = unsafe { cupsRasterReadHeader2(self.handle, &mut header) };
        if ret == 0 {
            None
        } else {
            Some(header)
        }
    }

    /// Read pixel data into buffer. Returns number of bytes read.
    pub fn read_pixels(&mut self, buf: &mut [u8]) -> usize {
        unsafe { cupsRasterReadPixels(self.handle, buf.as_mut_ptr(), buf.len() as c_uint) as usize }
    }
}

impl Drop for RasterReader {
    fn drop(&mut self) {
        unsafe { cupsRasterClose(self.handle) };
    }
}
