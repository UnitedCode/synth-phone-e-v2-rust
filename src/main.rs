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
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();
        rtt_init_print!();
        let system = system::System::init(ctx.core, ctx.device);
        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        rprintln!("Program Started");


        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
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
    #[task(binds = DMA1_STR1, local = [audio, buffer], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;

        if audio.get_stereo(buffer) {
            process_audio_buffer(buffer);

            for (left, right) in buffer {
                audio.push_stereo((*left, *right)).unwrap();
            }
        } else {
            rprintln!("Error reading data!");
        }
    }

    fn process_audio_buffer(buffer: &mut audio::AudioBuffer){
        for (left, right) in buffer.iter_mut() {
            let new_left = auto_tune(*left);
            let new_right = auto_tune(*right);
            *left = new_left;
            *right = new_right;
        }
    }

    fn auto_tune(sample: f32) -> f32 {
        let pitch = detect_pitch(sample);
        let corrected_pitch = correct_pitch(pitch);
        apply_pitch_correction(sample, corrected_pitch)
    }

    fn detect_pitch(sample: f32) -> f32{
        // TODO detect pitch
        sample
    }

    fn correct_pitch(pitch: f32) -> f32{
        // TODO correct pitch
        pitch
    }

    fn apply_pitch_correction(sample: f32, corrected_pitch: f32) -> f32 {
        // TODO apply corrected pitch
        corrected_pitch
    }
}
