//! Internal utility functions.

use std::ffi::c_char;

/// Copy a Rust byte string into a fixed-size `[c_char; N]` buffer,
/// NUL-terminating. Truncates if `src` is longer than `dst.len() - 1`.
pub fn copy_to_c_buf(dst: &mut [c_char], src: &[u8]) {
    let max = dst.len().saturating_sub(1);
    let copy_len = src.len().min(max);
    for (i, &b) in src[..copy_len].iter().enumerate() {
        dst[i] = b as c_char;
    }
    dst[copy_len] = 0;
}
