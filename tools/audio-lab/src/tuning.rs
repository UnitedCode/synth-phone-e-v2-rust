//! Independent equal-temperament oracle for the documented UI key order.
use crate::cents;

pub const KEY_TONICS: [usize; 24] = [
    0, 7, 2, 9, 4, 11, 6, 1, 5, 10, 3, 8, 9, 4, 11, 6, 1, 8, 2, 7, 0, 5, 10, 3,
];

pub fn nearest_scale_hz(hz: f64, key: usize) -> Option<f64> {
    if !hz.is_finite() || hz <= 0.0 || key >= KEY_TONICS.len() {
        return None;
    }
    let intervals = if key < 12 {
        [0, 2, 4, 5, 7, 9, 11]
    } else {
        [0, 2, 3, 5, 7, 8, 10]
    };
    (24..96)
        .filter(|midi| {
            intervals
                .iter()
                .any(|d| (KEY_TONICS[key] + d) % 12 == midi % 12)
        })
        .map(|midi| 440.0 * 2_f64.powf((midi as f64 - 69.0) / 12.0))
        .min_by(|a, b| cents(hz, *a).abs().total_cmp(&cents(hz, *b).abs()))
}
