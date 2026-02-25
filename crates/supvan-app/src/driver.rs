//! PAPPL printer driver callback and media helpers.

use std::ffi::{c_char, c_int, c_void, CStr};

use pappl_sys::*;

use crate::dither;
use crate::models::{self, DriverFamily};
use crate::raster;
use crate::util::copy_to_c_buf;

const MAX_PAPPL_MEDIA: usize = 256;

/// Find the best matching media index for given dimensions (in hundredths of mm).
pub fn find_best_media(family: &DriverFamily, w_hmm: c_int, h_hmm: c_int) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_dist = i64::MAX;
    for (i, sz) in family.media_sizes.iter().enumerate() {
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

/// Fill a `pappl_media_col_t` from media index within a family.
pub fn fill_media_col(family: &DriverFamily, col: &mut pappl_media_col_t, idx: usize) {
    copy_to_c_buf(&mut col.size_name, family.media_names[idx].to_bytes());
    col.size_width = family.media_sizes[idx][0];
    col.size_length = family.media_sizes[idx][1];
    col.left_margin = 0;
    col.right_margin = 0;
    col.top_margin = 0;
    col.bottom_margin = 0;
    copy_to_c_buf(&mut col.source, b"main-roll");
    copy_to_c_buf(&mut col.type_, b"labels");
}

/// PAPPL driver callback — configures driver data for any Supvan family.
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

    let name = CStr::from_ptr(driver_name);
    let family = match models::family_by_driver_name(name) {
        Some(f) => f,
        None => return false,
    };

    let d = &mut *data;

    // Make and model
    copy_to_c_buf(&mut d.make_and_model, &family.make_and_model);

    // Format: PWG raster
    d.format = c"application/vnd.cups-raster".as_ptr();
    d.orient_default = ipp_orient_e_IPP_ORIENT_PORTRAIT;
    d.quality_default = ipp_quality_e_IPP_QUALITY_NORMAL;

    // Resolution
    d.num_resolution = 1;
    d.x_resolution[0] = family.dpi;
    d.y_resolution[0] = family.dpi;
    d.x_default = family.dpi;
    d.y_default = family.dpi;

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
    let num_media = family.media_names.len().min(MAX_PAPPL_MEDIA) as c_int;
    d.num_media = num_media;
    for (i, name) in family.media_names.iter().enumerate().take(num_media as usize) {
        d.media[i] = name.as_ptr();
    }

    // Default media: first entry
    fill_media_col(family, &mut d.media_default, 0);

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

/// Auto-add callback: determine driver name from device_id MDL field.
pub unsafe extern "C" fn ks_autoadd_cb(
    _device_info: *const c_char,
    device_uri: *const c_char,
    device_id: *const c_char,
    _data: *mut c_void,
) -> *const c_char {
    if device_uri.is_null() {
        return std::ptr::null();
    }

    let uri = match CStr::from_ptr(device_uri).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null(),
    };

    if !uri.starts_with("btrfcomm://") && !uri.starts_with("usbhid://") {
        return std::ptr::null();
    }

    // Try to determine family from MDL in device_id
    if !device_id.is_null() {
        if let Ok(did) = CStr::from_ptr(device_id).to_str() {
            if let Some(mdl) = models::parse_mdl(did) {
                let family = models::family_for_model_hint(mdl);
                return family.driver_name.as_ptr();
            }
        }
    }

    // Fallback: T50 family
    models::default_family().driver_name.as_ptr()
}
