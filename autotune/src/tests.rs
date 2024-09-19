#[cfg(test)]
fn test() {
use libm::sinf;
use core::f64::consts::PI;


fn integration_test() {
    fn generate_sine_wave(frequency: f64, sample_rate: u32, duration: f64, buffer: &mut [f64]) -> usize {
        let num_samples = core::cmp::min(buffer.len(), (sample_rate as f64 * duration) as usize);
    
        for n in 0..num_samples {
            buffer[n] = sinf(2.0 * PI * frequency * n as f64 / sample_rate as f64);
        }
    
        num_samples
    }
    // TODO
    // Convert Sine to analysis Mag and Freq
    // Transform values
    // check that transformed values are what we expect
    // profit????????
}
}