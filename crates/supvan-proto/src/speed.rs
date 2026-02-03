/// Calculate print speed based on compressed buffer size.
///
/// From T50PlusPrint.multiCompression(): the speed is derived from the
/// average compressed bytes per buffer. Lower speed values for larger data
/// ensure the thermal head has enough time to heat properly.
pub fn calc_speed(compressed_size: usize) -> u16 {
    if compressed_size > 3000 {
        10
    } else if compressed_size > 2800 {
        15
    } else if compressed_size > 2500 {
        20
    } else if compressed_size > 2000 {
        25
    } else if compressed_size > 1500 {
        40
    } else if compressed_size > 1000 {
        45
    } else if compressed_size > 500 {
        55
    } else {
        60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_speed_thresholds() {
        assert_eq!(calc_speed(4000), 10);
        assert_eq!(calc_speed(3001), 10);
        assert_eq!(calc_speed(3000), 15);
        assert_eq!(calc_speed(2801), 15);
        assert_eq!(calc_speed(2800), 20);
        assert_eq!(calc_speed(2500), 25);
        assert_eq!(calc_speed(2000), 40);
        assert_eq!(calc_speed(1500), 45);
        assert_eq!(calc_speed(1000), 55);
        assert_eq!(calc_speed(500), 60);
        assert_eq!(calc_speed(100), 60);
        assert_eq!(calc_speed(0), 60);
    }
}
