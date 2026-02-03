use std::ffi::c_char;

/// Copy a Rust byte string into a fixed-size `[c_char; N]` buffer, NUL-terminating.
pub fn copy_to_c_buf(dst: &mut [c_char], src: &[u8]) {
    let max = dst.len().saturating_sub(1);
    let copy_len = src.len().min(max);
    for (i, &b) in src[..copy_len].iter().enumerate() {
        dst[i] = b as c_char;
    }
    dst[copy_len] = 0;
}

/// Return true if KATASYMBOL_MOCK=1.
pub fn is_mock_mode() -> bool {
    std::env::var("KATASYMBOL_MOCK")
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_to_c_buf_basic() {
        let mut buf = [0i8; 8];
        copy_to_c_buf(&mut buf, b"hello");
        assert_eq!(buf[0], b'h' as c_char);
        assert_eq!(buf[4], b'o' as c_char);
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn test_copy_to_c_buf_truncation() {
        let mut buf = [0i8; 4];
        copy_to_c_buf(&mut buf, b"hello world");
        assert_eq!(buf[0], b'h' as c_char);
        assert_eq!(buf[2], b'l' as c_char);
        assert_eq!(buf[3], 0); // NUL terminator
    }

    #[test]
    fn test_copy_to_c_buf_empty() {
        let mut buf = [0x7fi8; 4];
        copy_to_c_buf(&mut buf, b"");
        assert_eq!(buf[0], 0);
    }
}
