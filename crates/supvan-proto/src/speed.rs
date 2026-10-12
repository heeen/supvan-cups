//! The speed values sent as BUF_FULL's second parameter. The value tracks feed
//! rate in roughly mm/s.

/// Android `multiCompression`'s table: compressed bytes per print buffer above
/// the threshold → speed. Denser data burns slower.
const CALC_SPEED_STEPS: [(usize, u16); 7] = [
    (3000, 10),
    (2800, 15),
    (2500, 20),
    (2000, 25),
    (1500, 40),
    (1000, 45),
    (500, 55),
];

/// The table's ceiling — sparse data, and Electron's single-block pages.
pub(crate) const MAX_SPEED: u16 = 60;
/// Electron's speed for a page split over several blocks.
pub(crate) const SPLIT_PAGE_SPEED: u16 = 20;
/// The Android editor's forced speed for photos, below the table's floor.
const PHOTO_SPEED: u16 = 5;

/// Speed for a block averaging `bytes_per_buffer` compressed bytes per buffer.
pub(crate) fn calc_speed(bytes_per_buffer: usize) -> u16 {
    CALC_SPEED_STEPS
        .iter()
        .find(|&&(min, _)| bytes_per_buffer > min)
        .map_or(MAX_SPEED, |&(_, speed)| speed)
}

/// The fastest speed the vendor uses that doesn't exceed `mm_per_s`, bottoming
/// out at the photo speed.
pub(crate) fn fastest_at_most(mm_per_s: f32) -> u16 {
    std::iter::once(MAX_SPEED)
        .chain(CALC_SPEED_STEPS.iter().rev().map(|&(_, s)| s))
        .find(|&s| f32::from(s) <= mm_per_s)
        .unwrap_or(PHOTO_SPEED)
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

    #[test]
    fn fastest_at_most_walks_the_vendor_ladder() {
        assert_eq!(fastest_at_most(100.0), 60);
        assert_eq!(fastest_at_most(59.9), 55);
        assert_eq!(fastest_at_most(24.4), 20);
        assert_eq!(fastest_at_most(10.0), 10);
        assert_eq!(fastest_at_most(9.0), 5);
        assert_eq!(fastest_at_most(1.0), 5);
    }
}
