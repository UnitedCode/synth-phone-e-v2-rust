#![no_std]
#![no_main]
use panic_halt as _;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use libdaisy::audio;
    use libdaisy::system;
    use rtt_target::rprintln;
    use rtt_target::rtt_init_print;
    use synth_phone_e_v2_rust::logger;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        pitch_buffer: [f32; audio::BLOCK_SIZE_MAX],
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();
        rtt_init_print!();
        let system = system::System::init(ctx.core, ctx.device);
        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        let pitch_buffer = [0.0; audio::BLOCK_SIZE_MAX];
        rprintln!("Program Started");


        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
                pitch_buffer 
            },
            init::Monotonics(),
        )
    }

    // Non-default idle ensures chip doesn't go to sleep which causes issues for
    // probe.rs currently
    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        loop {
            cortex_m::asm::nop();
        }
    }

    // Interrupt handler for audio
    #[task(binds = DMA1_STR1, local = [audio, buffer, pitch_buffer], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;
        let pitch_buffer = ctx.local.pitch_buffer;

        if audio.get_stereo(buffer) {
            // Extract audio samples for processing
            for (left, right) in buffer.iter_mut() {
                let processed_sample = process_sample(*left, *right, pitch_buffer);
                *left = processed_sample.0;
                *right = processed_sample.1;
            }
            // Push processed audio back to the audio buffer
            for (left, right) in buffer.iter() {
                audio.push_stereo((*left, *right)).unwrap();
            }
        } else {
            rprintln!("Error reading data!");
        }
    }
    // Process a single audio sample for pitch detection and autotune
    fn process_sample(left: f32, right: f32, pitch_buffer: &mut [f32]) -> (f32, f32) {
        // Stub: Add sample to pitch buffer (left channel as example)
        add_sample_to_pitch_buffer(left, pitch_buffer);

        // Stub: Detect pitch from the pitch buffer
        let detected_pitch = detect_pitch(pitch_buffer);

        // Stub: Shift pitch to the desired pitch
        let (shifted_left, shifted_right) = shift_pitch(left, right, detected_pitch);

        (shifted_left, shifted_right)
    }

    // Stub: Add sample to pitch buffer
    fn add_sample_to_pitch_buffer(sample: f32, pitch_buffer: &mut [f32]) {
        // Shift the buffer to the left and add the new sample at the end
        pitch_buffer.rotate_left(1);
        pitch_buffer[pitch_buffer.len() - 1] = sample;
    }

    // Stub: Detect pitch from the pitch buffer
    fn detect_pitch(pitch_buffer: &[f32]) -> f32 {
        // Example: Return a fixed pitch (you need to implement actual pitch detection)
        440.0
    }

    // Stub: Shift pitch to the desired pitch
    fn shift_pitch(left: f32, right: f32, detected_pitch: f32) -> (f32, f32) {
        // Example: Return the original samples (you need to implement actual pitch shifting)
        (left, right)
    }
}
