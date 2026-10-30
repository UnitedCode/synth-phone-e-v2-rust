#![no_std]
#![no_main]

use core::{
    fmt::Write,
    sync::atomic::{AtomicU32, Ordering},
};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hio};
use synthphone_e_vocal_dsp::dsp::normalize_sample;

mod cases;
#[path = "../../../src/constants.rs"]
mod constants;
#[path = "../../audio-lab/src/replay.rs"]
mod replay;
#[path = "../../../src/vocal_frame.rs"]
mod vocal_frame;

use constants::{BLOCK_SIZE, FFT_SIZE, HOP_SIZE};
static TICKS: AtomicU32 = AtomicU32::new(0);

#[exception]
fn SysTick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    if let Ok(mut stderr) = hio::hstderr() {
        let _ = writeln!(stderr, "ARM guest failed: {info}");
    }
    debug::exit(debug::EXIT_FAILURE);
    loop {
        cortex_m::asm::bkpt();
    }
}

#[entry]
fn main() -> ! {
    static mut REPLAY: replay::Replay = replay::Replay::new();
    let mut core = cortex_m::Peripherals::take().unwrap();
    core.SYST
        .set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
    core.SYST.set_reload(20_000);
    core.SYST.enable_interrupt();
    core.SYST.enable_counter();

    let mut stdout = hio::hstdout().unwrap();
    stdout.write_all(cases::MAGIC).unwrap();
    stdout
        .write_all(&(cases::COUNT as u32).to_le_bytes())
        .unwrap();
    let config = vocal_frame::default_config();
    let total = (cases::INPUT_SAMPLES + 2 * FFT_SIZE).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    for case in 0..cases::COUNT {
        REPLAY.reset();
        stdout.write_all(&(case as u32).to_le_bytes()).unwrap();
        stdout.write_all(&(total as u32).to_le_bytes()).unwrap();
        let settings = cases::settings(case);
        let mut packet = [0_u8; HOP_SIZE * 4];
        let mut packet_samples = 0;
        for block in (0..total).step_by(BLOCK_SIZE) {
            for i in block..block + BLOCK_SIZE {
                let output = normalize_sample(REPLAY.sample(cases::input(case, i)), 0.8);
                assert!(
                    output.is_finite(),
                    "{} sample {} is not finite",
                    cases::NAMES[case],
                    i
                );
                packet[packet_samples * 4..packet_samples * 4 + 4]
                    .copy_from_slice(&output.to_le_bytes());
                packet_samples += 1;
                if packet_samples == HOP_SIZE {
                    stdout.write_all(&packet).unwrap();
                    packet_samples = 0;
                }
            }
            if REPLAY.pending() {
                let mut carrier = [0.0; HOP_SIZE];
                for (i, sample) in carrier.iter_mut().enumerate() {
                    *sample = cases::carrier(case, REPLAY.hops * HOP_SIZE + i);
                }
                let omit = if cfg!(feature = "omit-frame") && case == 4 {
                    Some(60)
                } else {
                    None
                };
                REPLAY.finish_block(&carrier, &config, &settings, omit);
            }
        }
        stdout.write_all(&packet[..packet_samples * 4]).unwrap();
        assert!(REPLAY.hops > 80);
    }
    let ticks = TICKS.load(Ordering::Relaxed);
    assert!(ticks > 0, "SysTick did not interrupt the DSP workload");
    stdout.write_all(&ticks.to_le_bytes()).unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        cortex_m::asm::bkpt();
    }
}
