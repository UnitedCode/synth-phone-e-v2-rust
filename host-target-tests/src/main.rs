use autotune::{
    circular_buffer::CircularBuffer,
    frequencies::find_nearest_note_frequency,
    hann_window::{self},
    process_frequencies::collect_harmonics,
};
use hound::{WavReader, WavSpec, WavWriter};
use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf};
use std::error::Error;
const PI: f32 = 3.14159265358979323846264338327950288f32;
const FFT_SIZE: usize = 1024;
const BUFFER_SIZE: usize = FFT_SIZE * 2;
const HOP_SIZE: usize = 128;

fn main() -> Result<(), Box<dyn Error>> {
    let path = "sweep.wav";
    // let path = "WeChooseToGoToTheMoon_f32.wav";
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    println!("Sample rate: {}", spec.sample_rate);

    match spec.sample_format {
        hound::SampleFormat::Int => match spec.bits_per_sample {
            _ => return Err(Box::from("Unsupported bit depth")),
        },
        hound::SampleFormat::Float => match spec.bits_per_sample {
            32 => read_and_write_samples::<f32>(&mut reader, &spec)?,
            _ => return Err(Box::from("Unsupported bit depth")),
        },
    }

    Ok(())
}

fn read_and_write_samples<S>(
    reader: &mut WavReader<std::io::BufReader<std::fs::File>>,
    spec: &WavSpec,
) -> Result<(), Box<dyn Error>>
where
    S: hound::Sample + std::fmt::Debug + hound::Sample,
{
    let output_spec = WavSpec { ..*spec };

    let output_path = "processed_sample.wav";
    let mut writer = WavWriter::create(output_path, output_spec)?;
    let mut buffer_in: CircularBuffer<f32, BUFFER_SIZE> = CircularBuffer::new(0.0, Some(0));
    let mut hop_counter = 0;
    let mut buffer_out: CircularBuffer<f32, BUFFER_SIZE> = CircularBuffer::new(0.0, Some(HOP_SIZE));

    let mut last_input_phases = [0.0; FFT_SIZE];
    let mut last_output_phases = [0.0; FFT_SIZE];
    let mut bin_frequencies = [0.0; FFT_SIZE / 2];

    for sample in reader.samples::<f32>() {
        let sample = sample.expect("Error reading sample");
        // println!("Sample: {:?}", sample);

        // Store the sample in the input buffer
        buffer_in.write(sample);

        // Read from the output buffer and reset the value
        let out_sample = buffer_out.read_and_reset();

        // Scale the output dow by the overlap factor
        let scaled_out_sample = out_sample * HOP_SIZE as f32 / FFT_SIZE as f32;

        // Increment the hop counter
        if hop_counter >= HOP_SIZE {
            hop_counter = 0;
            process_fft(
                &mut buffer_in,
                &mut buffer_out,
                &mut last_input_phases,
                &mut last_output_phases,
            );
            // update the output buffer write index to the start of the next hop
            // println!("-------- NEW HOP ------------------------");
            buffer_out.next_hop();
        }
        hop_counter += 1;
        writer.write_sample(scaled_out_sample)?;
    }

    writer.finalize()?;
    Ok(())
}

fn process_fft(
    in_buffer: &mut CircularBuffer<f32, BUFFER_SIZE>,
    out_buffer: &mut CircularBuffer<f32, BUFFER_SIZE>,
    last_input_phases: &mut [f32; FFT_SIZE],
    last_output_phases: &mut [f32; FFT_SIZE],
) {
    let bin_width = 44100 as f32 / FFT_SIZE as f32 * 2.0;
    let analysis_window_buffer: [f32; FFT_SIZE] = generate_hanning_window();
    let mut unwrapped_buffer: [f32; FFT_SIZE] = generate_hanning_window();

    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
        [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];

    let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
    let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
    let mut synthesis_magnitudes = [0.0; FFT_SIZE / 2];
    let mut synthesis_frequencies = [0.0; FFT_SIZE / 2];

    // copy buffer into FFT input, starting one window ago
    in_buffer.push_read_back(FFT_SIZE - HOP_SIZE);
    for n in 0..FFT_SIZE {
        unwrapped_buffer[n] *= in_buffer.read();
    }

    // Process the FFT based on the time domain input
    let fft = microfft::real::rfft_1024(&mut unwrapped_buffer);

    // ANALYSIS
    for i in 0..fft.len() {
        // Turn real and imaginary components into amplitude and phase
        let amplitude = sqrtf(fft[i].re * fft[i].re + fft[i].im * fft[i].im);
        let phase = atan2f(fft[i].im, fft[i].re);

        // //cut out noise
        // let magnitude_threshold = 0.05;
        // if amplitude < magnitude_threshold {
        //     continue; // Skip this bin if the magnitude is too low
        // }

        // Calculate the phase difference in this bin between the last
        // hop and this one, which will indirectly give us the exact frequency
        let mut phase_diff = phase - last_input_phases[i];

        // Subtract the amount of phase increment we'd expect to see based
        // on the centre frequency of this bin (2*pi*n/gFftSize) for this
        // hop size, then wrap to the range -pi to pi
        let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
        phase_diff = wrap_phase(phase_diff - bin_centre_frequency * HOP_SIZE as f32);

        // Find deviation from the centre frequency
        let bin_deviation = phase_diff * FFT_SIZE as f32 / HOP_SIZE as f32 / (2.0 * PI);

        // Add the original bin number to get the fractional bin where this partial belongs
        analysis_frequencies[i] = i as f32 + bin_deviation;

        // Save the magnitude for later
        analysis_magnitudes[i] = amplitude;
        // Save the phase for next hop
        last_input_phases[i] = phase;
    }

    // Zero out the synthesis bins, ready for new data (NOT done since it should already be zero)

    //TODO: maybe do this before analysis since (i believe) we should only shift the fundamental and harmonics
    //and if that is then we should not analyze noise/non-important freq
    let fundamental_index = find_fundamental_frequency(&analysis_magnitudes);
    println!(
        "- Exact frequency at bin {:<35}: {:<15} -  {:<10}",
        fundamental_index,
        analysis_frequencies[fundamental_index] * bin_width,
        analysis_magnitudes[fundamental_index]
    );
    let mut max_magnitude = 0.0;
    let mut fundamental_index = 0;
    for (i, &magnitude) in analysis_magnitudes.iter().enumerate() {
        if magnitude > max_magnitude {
            max_magnitude = magnitude;
            fundamental_index = i;
        }
        // println!("i:{i:<10} current_mag:{magnitude:<20} max_mag:{max_magnitude:<20} bin: {fundamental_index:<10} freq::{:<10}", analysis_frequencies[i]);
    }
    let harmonics = collect_harmonics(fundamental_index);

    //TODO: just pitch shift the fundamental and the harmonics by the same amount
    // Handle the pitch shift, storing frequencies into new bins
    // let exact_frequency = analysis_frequencies[fundamental_index];
    // let target_frequency = find_nearest_note_frequency(exact_frequency);
    // println!("Target {target_frequency} exact {exact_frequency} fund_index {fundamental_index}");
    // let pitch_shift_ratio = target_frequency / exact_frequency;

    for i in 0..FFT_SIZE / 2 {
        let new_bin = 0;
        // let new_bin = floorf(i as f32 * pitch_shift_ratio + 0.5) as usize;
        // if new_bin < FFT_SIZE / 2 {
        // println!("pre bin: {new_bin:<6} am: {:<15} af: {:<15}",synthesis_magnitudes[i], analysis_frequencies[i]);

        synthesis_magnitudes[i] = analysis_magnitudes[i];

        synthesis_frequencies[i] = analysis_frequencies[i];
        // println!(
        //     "bin: {i:<10} am: {:<15} af: {:<15}",
        //     analysis_magnitudes[i], analysis_frequencies[i]
        // );
        // }
    }

    // SYNTHESIS
    for i in 0..FFT_SIZE / 2 {
        let amplitude = synthesis_magnitudes[i];
        // Get the fractional offset from the bin centre frequency
        let bin_deviation = synthesis_frequencies[i] - i as f32;

        // Multiply to get back to a phase value
        let mut phase_diff = bin_deviation * 2.0 * PI * HOP_SIZE as f32 / FFT_SIZE as f32;

        // Add the expected phase increment based on the bin centre frequency
        let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
        phase_diff += bin_centre_frequency * HOP_SIZE as f32;

        // Advance the phase from the previous hop
        let out_phase = wrap_phase(last_output_phases[i] + phase_diff);

        // Now convert magnitude and phase back to real and imaginary components
        fft[i].re = amplitude * cosf(out_phase);
        fft[i].im = amplitude * sinf(out_phase);

        // Also store the complex conjugate in the upper half of the spectrum
        full_spectrum[i] = fft[i]; // First half directly
        if i > 0 && i < (FFT_SIZE / 2) {
            full_spectrum[FFT_SIZE - i] = fft[i].conj(); // Conjugate symmetry for the second half
        }

        // Save the phase for the next hop
        last_output_phases[i] = out_phase;
    }

    // Run the inverse FFT
    let res = microfft::inverse::ifft_1024(&mut full_spectrum);

    // Add time domain into the output buffer
    for (n, val) in res.iter().enumerate() {
        let windowed_val = val.re * analysis_window_buffer[n]; // Window again and scale
        out_buffer.add_value(windowed_val);
    }
}

fn wrap_phase(phase_in: f32) -> f32 {
    if phase_in >= 0.0 {
        return fmodf(phase_in + PI, 2.0 * PI) - PI;
    }
    fmodf(phase_in - PI, -2.0 * PI) + PI
}

pub fn find_fundamental_frequency(analysis_magnitudes: &[f32]) -> usize {
    let mut max_magnitude = 0.0;
    let mut fundamental_bin = 0;
    for (i, &magnitude) in analysis_magnitudes.iter().enumerate() {
        if magnitude > max_magnitude {
            max_magnitude = magnitude;
            fundamental_bin = i;
        }
        // println!("i:{i:<10} current_mag:{magnitude:<20} max_mag:{max_magnitude:<20} bin: {fundamental_bin:>10}");
    }
    fundamental_bin
}

pub fn generate_hanning_window() -> [f32; FFT_SIZE] {
    let mut window = [0.0; FFT_SIZE];
    for n in 0..FFT_SIZE {
        window[n] = 0.5 * (1.0 - cosf(2.0 * PI * n as f32 / (FFT_SIZE - 1) as f32));
    }
    window
}
