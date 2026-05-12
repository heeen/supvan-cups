//! PAPPL printer driver callback and media helpers.

use std::ffi::{c_char, c_int, c_void, CStr};

use pappl_sys::*;

use crate::dither;
use crate::models::{self, DriverFamily};
use crate::raster;
use crate::util::copy_to_c_buf;

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

    pappl_rs::DriverDataBuilder::new()
        .make_and_model(&family.make_and_model)
        .format(c"application/vnd.cups-raster")
        .orient_default(ipp_orient_e_IPP_ORIENT_PORTRAIT)
        .quality_default(ipp_quality_e_IPP_QUALITY_NORMAL)
        .ppm(1)
        .resolution(family.dpi)
        .monochrome()
        .label_printer()
        .darkness(50, 16)
        .speed(0, [0, 0])
        .media(&family.media_names, &family.media_sizes)
        .dither_bayer4(&dither::BAYER4)
        .raster_callbacks(
            Some(raster::ks_rstartjob_cb),
            Some(raster::ks_rstartpage_cb),
            Some(raster::ks_rwriteline_cb),
            Some(raster::ks_rendpage_cb),
            Some(raster::ks_rendjob_cb),
            Some(raster::ks_status_cb),
        )
        .build(&mut *data);

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
