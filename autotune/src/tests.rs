#[cfg(test)]
fn test() {
    use std::f64::consts::PI;

fn generate_sine_wave(frequency: f64, sample_rate: u32, duration: f64) -> Vec<f64> {
    let num_samples = (sample_rate as f64 * duration) as usize;
    let mut sine_wave = Vec::with_capacity(num_samples);

    for n in 0..num_samples {
        let sample = (2.0 * PI * frequency * n as f64 / sample_rate as f64).sin();
        sine_wave.push(sample);
    }

    sine_wave
}
    // TODO
    // Convert Sine to analysis Mag and Freq
    // Transform values
    // check that transformed values are what we expect
    // profit????????
}
