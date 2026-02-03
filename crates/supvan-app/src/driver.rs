//! PAPPL printer driver callback and media table.

use std::ffi::{c_char, c_int, c_void};

use pappl_sys::*;

use crate::dither;
use crate::raster;
use crate::util::copy_to_c_buf;

pub static DRIVER_NAME: &std::ffi::CStr = c"supvan_t50pro";

const MAX_PAPPL_MEDIA: usize = 256;

/// PWG self-describing media size names.
pub static MEDIA_NAMES: &[&std::ffi::CStr] = &[
    c"oe_40x30mm_40x30mm",
    c"oe_40x40mm_40x40mm",
    c"oe_40x50mm_40x50mm",
    c"oe_40x60mm_40x60mm",
    c"oe_40x70mm_40x70mm",
    c"oe_40x80mm_40x80mm",
    c"oe_30x15mm_30x15mm",
    c"oe_30x20mm_30x20mm",
    c"oe_30x30mm_30x30mm",
    c"oe_30x40mm_30x40mm",
    c"oe_48x30mm_48x30mm",
    c"oe_48x45mm_48x45mm",
    c"oe_48x70mm_48x70mm",
    c"oe_25x25mm_25x25mm",
    c"oe_50x30mm_50x30mm",
];

/// Dimensions in hundredths of mm (width, length).
pub static MEDIA_SIZES: &[[c_int; 2]] = &[
    [4000, 3000],
    [4000, 4000],
    [4000, 5000],
    [4000, 6000],
    [4000, 7000],
    [4000, 8000],
    [3000, 1500],
    [3000, 2000],
    [3000, 3000],
    [3000, 4000],
    [4800, 3000],
    [4800, 4500],
    [4800, 7000],
    [2500, 2500],
    [5000, 3000],
];

/// Find the best matching media index for given dimensions (in hundredths of mm).
pub fn find_best_media(w_hmm: c_int, h_hmm: c_int) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_dist = i64::MAX;
    for (i, sz) in MEDIA_SIZES.iter().enumerate() {
        let dw = (sz[0] - w_hmm) as i64;
        let dh = (sz[1] - h_hmm) as i64;
        let dist = dw * dw + dh * dh;
        if dist < best_dist {
            best_dist = dist;
            best = Some(i);
        }
    }
    best
}

/// Fill a `pappl_media_col_t` from media index.
pub fn fill_media_col(col: &mut pappl_media_col_t, idx: usize) {
    copy_to_c_buf(&mut col.size_name, MEDIA_NAMES[idx].to_bytes());
    col.size_width = MEDIA_SIZES[idx][0];
    col.size_length = MEDIA_SIZES[idx][1];
    col.left_margin = 0;
    col.right_margin = 0;
    col.top_margin = 0;
    col.bottom_margin = 0;
    copy_to_c_buf(&mut col.source, b"main-roll");
    copy_to_c_buf(&mut col.type_, b"labels");
}

/// PAPPL driver callback — configures driver data for our printer.
pub unsafe extern "C" fn ks_driver_cb(
    _system: *mut pappl_system_t,
    driver_name: *const c_char,
    _device_uri: *const c_char,
    _device_id: *const c_char,
    data: *mut pappl_pr_driver_data_t,
    _attrs: *mut *mut ipp_t,
    _cbdata: *mut c_void,
) -> bool {
    if driver_name.is_null() || data.is_null() {
        return false;
    }

    let name = std::ffi::CStr::from_ptr(driver_name);
    if name != DRIVER_NAME {
        return false;
    }

    let d = &mut *data;

    // Make and model
    copy_to_c_buf(&mut d.make_and_model, b"Supvan T50 Pro");

    // Format: PWG raster
    d.format = c"application/vnd.cups-raster".as_ptr();
    d.orient_default = ipp_orient_e_IPP_ORIENT_PORTRAIT;
    d.quality_default = ipp_quality_e_IPP_QUALITY_NORMAL;

    // Resolution: 203x203 DPI
    d.num_resolution = 1;
    d.x_resolution[0] = 203;
    d.y_resolution[0] = 203;
    d.x_default = 203;
    d.y_default = 203;

    // Raster type
    d.raster_types = pappl_raster_type_e_PAPPL_PWG_RASTER_TYPE_BLACK_1;
    d.color_supported = pappl_color_mode_e_PAPPL_COLOR_MODE_MONOCHROME;
    d.color_default = pappl_color_mode_e_PAPPL_COLOR_MODE_MONOCHROME;

    // One-sided only
    d.sides_supported = pappl_sides_e_PAPPL_SIDES_ONE_SIDED;
    d.sides_default = pappl_sides_e_PAPPL_SIDES_ONE_SIDED;

    // Label printer
    d.borderless = true;
    d.left_right = 0;
    d.bottom_top = 0;
    d.kind = pappl_kind_e_PAPPL_KIND_LABEL;
    d.has_supplies = true;

    // Darkness
    d.darkness_configured = 50;
    d.darkness_supported = 16;

    // Label mode
    d.mode_configured = pappl_label_mode_e_PAPPL_LABEL_MODE_TEAR_OFF as _;
    d.mode_supported = pappl_label_mode_e_PAPPL_LABEL_MODE_TEAR_OFF as _;

    // PPM
    d.ppm = 1;

    // Speed
    d.speed_default = 0;
    d.speed_supported[0] = 0;
    d.speed_supported[1] = 0;

    // Media sizes
    let num_media = MEDIA_NAMES.len().min(MAX_PAPPL_MEDIA) as c_int;
    d.num_media = num_media;
    for (i, name) in MEDIA_NAMES.iter().enumerate().take(num_media as usize) {
        d.media[i] = name.as_ptr();
    }

    // Default media: 40x30mm
    fill_media_col(&mut d.media_default, 0);

    // Sources / types
    d.num_source = 1;
    d.source[0] = c"main-roll".as_ptr();
    d.num_type = 1;
    d.type_[0] = c"labels".as_ptr();

    // Media-ready: same as default
    d.media_ready[0] = d.media_default;

    // 16x16 dither matrix (tiled from 4x4 Bayer)
    for r in 0..16 {
        for c in 0..16 {
            d.gdither[r][c] = dither::BAYER4[r % 4][c % 4];
        }
    }
    d.pdither = d.gdither;

    // Raster callbacks
    d.rstartjob_cb = Some(raster::ks_rstartjob_cb);
    d.rstartpage_cb = Some(raster::ks_rstartpage_cb);
    d.rwriteline_cb = Some(raster::ks_rwriteline_cb);
    d.rendpage_cb = Some(raster::ks_rendpage_cb);
    d.rendjob_cb = Some(raster::ks_rendjob_cb);

    // Status callback
    d.status_cb = Some(raster::ks_status_cb);

    true
}

/// Auto-add callback: return our driver name for btrfcomm:// devices.
pub unsafe extern "C" fn ks_autoadd_cb(
    _device_info: *const c_char,
    device_uri: *const c_char,
    _device_id: *const c_char,
    _data: *mut c_void,
) -> *const c_char {
    if device_uri.is_null() {
        return std::ptr::null();
    }

    let uri = std::ffi::CStr::from_ptr(device_uri);
    if let Ok(s) = uri.to_str() {
        if s.starts_with("btrfcomm://") {
            return DRIVER_NAME.as_ptr();
        }
    }
    std::ptr::null()
}
