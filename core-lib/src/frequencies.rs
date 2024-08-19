use libm::fabsf;
use log::warn;

pub const FREQUENCIES: [f32; 142] = [
    16.35, 17.32, 17.32, 18.35, 19.45, 19.45, 20.60, 21.83, 23.12, 23.12, 24.50, 25.96, 25.96,
    27.50, 29.14, 29.14, 30.87, 32.70, 34.65, 34.65, 36.71, 38.89, 38.89, 41.20, 43.65, 46.25,
    46.25, 49.00, 51.91, 51.91, 55.00, 58.27, 58.27, 61.74, 65.41, 69.30, 69.30, 73.42, 77.78,
    77.78, 82.41, 87.31, 92.50, 92.50, 98.00, 103.83, 103.83, 110.00, 116.54, 116.54, 123.47,
    130.81, 138.59, 138.59, 146.83, 155.56, 155.56, 164.81, 174.61, 185.00, 185.00, 196.00, 207.65,
    207.65, 220.00, 233.08, 233.08, 246.94, 261.63, 277.18, 277.18, 293.66, 311.13, 311.13, 329.63,
    349.23, 369.99, 369.99, 392.00, 415.30, 415.30, 440.00, 466.16, 466.16, 493.88, 523.25, 554.37,
    554.37, 587.33, 622.25, 622.25, 659.26, 698.46, 739.99, 739.99, 783.99, 830.61, 830.61, 880.00,
    932.33, 932.33, 987.77, 1046.50, 1108.73, 1108.73, 1174.66, 1244.51, 1244.51, 1318.51, 1396.91,
    1479.98, 1479.98, 1567.98, 1661.22, 1661.22, 1760.00, 1864.66, 1864.66, 1975.53, 2093.00,
    2217.46, 2217.46, 2349.32, 2489.02, 2489.02, 2637.02, 2793.83, 2959.96, 2959.96, 3135.96,
    3322.44, 3322.44, 3520.00, 3729.31, 3729.31, 3951.07, 4186.01, 4434.92, 4434.92, 4698.64,
    4978.03, 4978.03,
];

#[inline(always)]
pub fn find_nearest_note_frequency(frequency: f32) -> f32 {
    if FREQUENCIES.is_empty() {
        warn!("COULDN'T FIND FREQUENCY");
        return frequency;
    }

    let mut low = 0;
    let mut high = FREQUENCIES.len() - 1;

    while low < high {
        let mid = (low + high) / 2;
        let mid_freq = FREQUENCIES[mid];

        if mid_freq < frequency {
            low = mid + 1;
        } else {
            high = mid;
        }
    }

    // After the loop, 'low' should be the index of the closest frequency or the next higher frequency.
    // Check if the previous frequency is closer.
    if low > 0 && fabsf(FREQUENCIES[low] - frequency) > fabsf(FREQUENCIES[low - 1] - frequency) {
        low -= 1;
    }

    FREQUENCIES[low]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_nearest_note_frequency_exact_match() {
        let frequency = 440.0;
        let expected = 440.0;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_in_between() {
        let frequency = 445.0;
        let expected = 440.0;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_below_range() {
        let frequency = 10.0;
        let expected = 16.35;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_above_range() {
        let frequency = 5000.0;
        let expected = 4978.03;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_mid_point() {
        let frequency = 55.0;
        let expected = 55.0;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_edge_case_low() {
        let frequency = 16.0;
        let expected = 16.35;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_edge_case_high() {
        let frequency = 4999.0;
        let expected = 4978.03;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_very_close_lower() {
        let frequency = 110.1;
        let expected = 110.0;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_nearest_note_frequency_very_close_upper() {
        let frequency = 109.9;
        let expected = 110.0;
        let result = find_nearest_note_frequency(frequency);
        assert_eq!(result, expected);
    }
}
