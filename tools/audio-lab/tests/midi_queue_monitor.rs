use audio_lab::{
    constants::{BLOCK_SIZE, HOP_SIZE},
    midi_events::{get_midi_queue_status, MidiEvent, MidiQueueMonitor},
};
use heapless::spsc::Queue;

fn queue(len: usize) -> Queue<MidiEvent, 128> {
    let mut queue = Queue::new();
    for _ in 0..len {
        queue.enqueue(MidiEvent::Other).unwrap();
    }
    queue
}

#[test]
fn queue_warnings_run_every_4800_samples_despite_fft_counter_resets() {
    let queue = queue(127);
    let mut monitor = MidiQueueMonitor::new();
    let mut hop_counter = 0;
    let mut warnings = Vec::new();
    let mut legacy_checks = 0;
    let mut occupancy_reads = 0;
    for block in 1..=48_000 / BLOCK_SIZE {
        for _ in 0..BLOCK_SIZE {
            if hop_counter >= HOP_SIZE {
                hop_counter = 0;
            }
            hop_counter += 1;
        }
        if hop_counter % 4_800 == 0 {
            legacy_checks += 1;
        }
        let warning = monitor.on_audio_block(|| {
            occupancy_reads += 1;
            get_midi_queue_status(&queue)
        });
        if let Some(status) = warning {
            assert_eq!(status, (127, 127));
            warnings.push(block * BLOCK_SIZE);
        }
    }
    assert_eq!(legacy_checks, 0, "the old FFT-based deadline never runs");
    assert_eq!(
        warnings,
        (1..=10).map(|period| period * 4_800).collect::<Vec<_>>()
    );
    assert_eq!(
        occupancy_reads, 10,
        "do not lock/read the queue on every block"
    );
}

#[test]
fn queue_occupancy_threshold_is_strict_and_warning_rate_is_bounded() {
    for len in [0, 95, 96, 127] {
        let queue = queue(len);
        let mut monitor = MidiQueueMonitor::new();
        let mut reads = 0;
        let mut warnings = 0;
        for _ in 0..9_600 / BLOCK_SIZE {
            if let Some(status) = monitor.on_audio_block(|| {
                reads += 1;
                get_midi_queue_status(&queue)
            }) {
                assert_eq!(status, (len, 127));
                warnings += 1;
            }
        }
        assert_eq!(reads, 2);
        assert_eq!(
            warnings,
            if len >= 96 { 2 } else { 0 },
            "queue length {len}"
        );
    }
}

#[test]
fn recovery_and_refill_are_observed_on_the_next_deadline() {
    let mut queue = queue(127);
    let mut monitor = MidiQueueMonitor::default();
    let mut warnings = Vec::new();
    for block in 1..=14_400 / BLOCK_SIZE {
        if block == 4_800 / BLOCK_SIZE + 1 {
            while queue.dequeue().is_some() {}
        }
        if block == 9_600 / BLOCK_SIZE + 1 {
            for _ in 0..96 {
                queue.enqueue(MidiEvent::Other).unwrap();
            }
        }
        if let Some(status) = monitor.on_audio_block(|| get_midi_queue_status(&queue)) {
            warnings.push((block * BLOCK_SIZE, status));
        }
    }
    assert_eq!(warnings, [(4_800, (127, 127)), (14_400, (96, 127))]);
}
