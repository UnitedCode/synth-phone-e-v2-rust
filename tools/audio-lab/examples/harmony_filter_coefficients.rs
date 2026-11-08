//! Reproduce the DSP's 65-tap, unity-DC Hamming/sinc pitch-decimation filter.
fn main() {
    let cutoff = 1.0_f64 / 32.0;
    let mut taps = Vec::new();
    for i in 0..65 {
        let x = i as f64 - 32.0;
        let sinc = if x == 0.0 {
            2.0 * cutoff
        } else {
            (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
        };
        let window = 0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / 64.0).cos();
        taps.push(sinc * window);
    }
    let sum: f64 = taps.iter().sum();
    for tap in taps {
        println!("{:.12},", tap / sum);
    }
}
