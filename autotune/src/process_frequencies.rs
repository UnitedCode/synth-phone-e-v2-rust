use libm::floorf;

use crate::frequencies::find_nearest_note_frequency;

//TODO this should be passed in
const FFT_SIZE: usize = 1024;

#[inline(always)]
pub fn calculate_updates(
    index: usize,
    analysis_frequencies: &[f32],
    analysis_magnitudes: &[f32],
    transition_speed: f32,
) -> Option<(usize, f32, f32)> {
    if index >= analysis_frequencies.len() || index >= analysis_magnitudes.len() {
        return None;
    }
    let exact_frequency = analysis_frequencies[index];
    let target_frequency = find_nearest_note_frequency(exact_frequency);
    let pitch_shift = target_frequency / exact_frequency;

    let new_bin = floorf(index as f32 * pitch_shift + 0.5) as usize;

    if new_bin < FFT_SIZE / 2 {
        let updated_magnitude = transition_speed * analysis_magnitudes[new_bin]
            + (1.0 - transition_speed) * analysis_magnitudes[index];
        let updated_frequency = exact_frequency * pitch_shift;
        Some((new_bin, updated_magnitude, updated_frequency))
    } else {
        None
    }
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

    #[test]
    fn test_calculate_updates_within_bounds() {
        let analysis_frequencies = vec![440.0, 880.0, 1760.0];
        let analysis_magnitudes = vec![1.0, 0.5, 0.25];
        let transition_speed = 0.1;

        let result = calculate_updates(
            0,
            &analysis_frequencies,
            &analysis_magnitudes,
            transition_speed,
        );
        assert!(result.is_some());
        let (new_bin, updated_magnitude, updated_frequency) = result.unwrap();
        assert_eq!(new_bin, 0);
        assert!((updated_magnitude - 1.0).abs() < 1e-6);
        assert!((updated_frequency - 440.0).abs() < 1e-6);

        let result = calculate_updates(
            1,
            &analysis_frequencies,
            &analysis_magnitudes,
            transition_speed,
        );
        assert!(result.is_some());
        let (new_bin, updated_magnitude, updated_frequency) = result.unwrap();
        assert_eq!(new_bin, 1);
        assert!((updated_magnitude - 0.5).abs() < 1e-6);
        assert!((updated_frequency - 880.0).abs() < 1e-6);
    }

    #[test]
    fn test_calculate_updates_out_of_bounds() {
        let analysis_frequencies = vec![440.0, 880.0, 1760.0];
        let analysis_magnitudes = vec![1.0, 0.5, 0.25];
        let transition_speed = 0.1;

        // This index should be out of bounds
        let result = calculate_updates(
            FFT_SIZE / 2,
            &analysis_frequencies,
            &analysis_magnitudes,
            transition_speed,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_calculate_updates_with_transition() {
        let analysis_frequencies = vec![440.0, 880.0, 1760.0];
        let analysis_magnitudes = vec![1.0, 0.5, 0.25];
        let transition_speed = 0.5;

        let result = calculate_updates(
            0,
            &analysis_frequencies,
            &analysis_magnitudes,
            transition_speed,
        );
        assert!(result.is_some());
        let (new_bin, updated_magnitude, updated_frequency) = result.unwrap();
        assert_eq!(new_bin, 0);
        assert!((updated_magnitude - 1.0).abs() < 1e-6);
        assert!((updated_frequency - 440.0).abs() < 1e-6);

        let result = calculate_updates(
            1,
            &analysis_frequencies,
            &analysis_magnitudes,
            transition_speed,
        );
        assert!(result.is_some());
        let (new_bin, updated_magnitude, updated_frequency) = result.unwrap();
        assert_eq!(new_bin, 1);
        assert!((updated_magnitude - 0.5).abs() < 1e-6);
        assert!((updated_frequency - 880.0).abs() < 1e-6);
    }
}
