# Performance Recommendations

Findings from a review of the firmware on 2026-07-05, expanded 2026-07-06. Binary size
at time of review: **111.5 KB text of 128 KB flash (~85% full)**, so size headroom
constrains optimization choices. Ordered by expected payoff.

**Companion doc:** `synthphone_e_vocal_dsp/docs/performance-recommendations.md`. Most
of the CPU per second is spent *inside the DSP crate* (per-bin `libm` calls in the
phase vocoder, the cepstral envelope's 2 extra FFTs per hop, `libm::sinf` per sample in
the oscillator). Implement that doc's P1–P5 alongside this one — the two docs together
are one plan. Note the DSP crate is pinned by git commit in `Cargo.toml`; switch to the
commented-out path dependency while iterating, and bump the pin when done.

## Runtime performance

### 1. Add `-C target-cpu=cortex-m7` to rustflags (free win)

`.cargo/config.toml` only sets the linker arg. Without `target-cpu`, LLVM schedules for a
generic thumbv7em core and misses M7-specific dual-issue scheduling and FPv5 features:

```toml
[target.thumbv7em-none-eabihf]
rustflags = [
    "-C", "link-arg=-Tlink.x",
    "-C", "target-cpu=cortex-m7",
]
```

### 2. Raise `BLOCK_SIZE` from 2 and hoist locks out of the per-sample loop

Likely the biggest structural win. With `BLOCK_SIZE = 2`, `update_handler` fires at
~24 kHz. Every interrupt pays context save/restore plus FPU lazy stacking, and inside the
loop each sample takes 4–6 RTIC locks (`in_ring`, `out_ring`, `midi_events`,
`voice_manager` — see `src/handler.rs`).

- A `BLOCK_SIZE` of 32–64 cuts interrupt overhead 16–32×.
- Restructure to lock each resource once per block: drain MIDI once, then synthesize the
  whole block. This removes tens of thousands of lock/unlock pairs per second.

Cost: ~0.7–1.3 ms extra I/O latency — small next to the FFT hop latency (256 samples
≈ 5.3 ms) already in the vocal path.

#### Implementation spec

Prerequisite: `push_slice` / `pop_slice` on the DSP crate's `RingBuffer` (companion doc
item P6).

Constraints on the new `BLOCK_SIZE` (in `src/constants.rs`):

- must divide `HOP_SIZE` (256) evenly so hop boundaries land exactly on block
  boundaries — 32 or 64 both work; start with 32;
- it flows automatically into `libdaisy::system_init!(BLOCK_SIZE)` and the
  `audio::AudioBuffer` local (already sized to `BLOCK_SIZE_MAX`), so no other change is
  needed for the DMA setup.

Restructure `audio_handler` (`src/handler.rs`) into phases, each locking a resource at
most once:

```rust
// Phase 0 — settings snapshot: unchanged (already one lock per interrupt).

// Phase 1 — split input into a local array (no locks; buffer is a local).
let mut input = [0.0f32; BLOCK_SIZE];
for (i, (left, right)) in buffer.as_slice()[..BLOCK_SIZE].iter().enumerate() {
    input[i] = if is_hangup_button_pressed { *left } else { *right };
}
shared.in_ring.lock(|r| r.push_slice(&input)); // ONE lock, one atomic store

// Phase 2 — drain MIDI once per block (not 2 events per sample).
// Keep the existing match body verbatim; just move it out of the sample loop and
// raise the per-call budget: up to 16 events per block.
shared.midi_events.lock(|events| {
    if events.is_empty() { return; }
    shared.voice_manager.lock(|vm| {
        vm.set_waveform(wave_type);
        for _ in 0..16 {
            let Some(event) = events.dequeue() else { break };
            /* existing match on event — unchanged */
        }
    });
});

// Phase 3 — synthesize the whole block of MIDI audio under ONE voice_manager lock.
let mut midi_block = [0.0f32; BLOCK_SIZE];
shared.voice_manager.lock(|vm| {
    for s in midi_block.iter_mut() {
        *s = vm.get_mixed_sample();
    }
});

// Phase 4 — pop the processed vocal block under ONE out_ring lock.
let mut out_block = [0.0f32; BLOCK_SIZE];
shared.out_ring.lock(|r| r.pop_slice(&mut out_block));

// Phase 5 — pure per-sample math on locals, no locks:
for i in 0..BLOCK_SIZE {
    let mut s = sample_rate_reduce(out_block[i], sr_factor, sr_hold_counter, sr_held_value);
    s = bitcrush(s, bit_depth as i8);
    s += (midi_block[i] * waveform_compensation) * 0.1;
    s = normalize_sample(s, 0.8) * volume_gain;
    if audio.push_stereo((s, s)).is_err() { warn!("Failed to write audio data"); }
}

// Phase 6 — hop bookkeeping, once per block (BLOCK_SIZE divides HOP_SIZE, so the
// boundary is exact):
*hop_counter += BLOCK_SIZE as u32;
if *hop_counter >= HOP_SIZE as u32 {
    *hop_counter = 0;
    let pointer = shared.in_ring.lock(|r| r.write_index());
    shared.in_pointer_cached.lock(|c| *c = pointer);
    if crate::rtic_app::app::dma1_stream0_fft_task::spawn().is_err() {
        warn!("Could not spawn FFT task - underrun error");
    }
}
```

Behavioral notes for the implementer:

- Ordering within a block does not change observable behavior: `update_handler` is the
  highest-priority task (8), so the FFT task (7) could never interleave mid-loop even
  in the old code.
- MIDI events now land at block granularity (~0.7 ms at BLOCK_SIZE=32) instead of
  2-per-sample — indistinguishable, since MIDI bytes arrive at 31.25 kbps (~1 ms per
  3-byte message) anyway.
- Verify after the change: audio passthrough clean, MIDI notes trigger, hop cadence
  unchanged (FFT task still spawns every 256 samples — check with an RTT counter).

### 3. Per-crate `opt-level = 3` for DSP hot spots, keep `"s"` globally

~19 KB of flash headroom exists. A global `opt-level = 3` won't fit, but the crates doing
per-sample and FFT math can be optimized individually:

```toml
[profile.release.package.synthphone-e-vocal-dsp]
opt-level = 3
[profile.release.package.microfft]
opt-level = 3
```

`opt-level = "s"` disables loop vectorization/unrolling, which hurts FFT and
phase-vocoder loops the most. Measure the size delta after enabling.

### 4. Bigger move: escape the 128 KB flash ceiling with the Daisy bootloader

The H750 has only 128 KB internal flash, but the board has 8 MB QSPI and 64 MB SDRAM
unused. The Electro-Smith Daisy bootloader can boot an app from QSPI (executed from
SRAM). That would allow building the entire firmware at `opt-level = 3` and running code
from zero-wait-state SRAM instead of flash. Long-term fix if features keep growing.

### 5. ITCM is defined but unused (64 KB, `memory.x`)

Placing FFT/audio handler code there via `#[link_section]` plus a startup copy avoids
flash wait states entirely. libdaisy already enables I-cache and D-cache
(`system.rs:413`), so gains are moderate once the cache is warm — only worth doing after
items 1–3 if profiling still shows pressure.

## Build / iteration speed

### 6. Relax the dev profile

The dev profile has `lto = true`, `codegen-units = 1`, `opt-level = "s"` — every
`cargo test` and debug build pays full release-grade link times. If only `--release`
builds are flashed:

```toml
[profile.dev]
lto = false
codegen-units = 16
```

Keep `opt-level = "s"` if dev builds are flashed; otherwise drop it too.

## Housekeeping

### 7a. MIDI queue monitor is dead code — it can never fire

`src/handler.rs:182`: `if *hop_counter % 4800 == 0`. But `hop_counter` is reset to 0
whenever it reaches `HOP_SIZE` (256) and then immediately incremented, so at the point
of this check it is always in 1..=256 — `% 4800 == 0` is never true and the queue
warning never runs (the comment "every ~100ms" is wrong). Fix by giving the monitor its
own counter:

```rust
// new local resource: monitor_counter: u32 (init 0)
*monitor_counter += BLOCK_SIZE as u32;
if *monitor_counter >= 4800 {
    *monitor_counter = 0;
    /* existing queue-status check */
}
```

Or delete the check entirely — `try_enqueue_midi_event` already logs drops.

### 7b. Display I2C runs at 100 kHz — flushes take ~45 ms

`src/main.rs:206` configures I2C1 at `100_u32.kHz()`. A full SSD1306 flush moves
~512 bytes + command overhead ≈ 45 ms per screen update at 100 kHz. It runs at
priority 2 so audio is unaffected, but UI updates lag noticeably. The SSD1306 supports
400 kHz fast mode — change to `400_u32.kHz()`. One-line change; verify the display
still initializes (marginal wiring can force 100 kHz, in which case revert).

### 7. Delete dead modules

The compiler confirms these are never constructed:

- `src/midi/midi_voice.rs` — old `VoiceManager` with a per-sample `libm::sqrtf`,
  superseded by `voice_generator.rs` (which correctly uses an `INV_SQRT` lookup table)
- `src/audio/sample_player.rs`

LTO strips them from the binary, but they cost compile time and are a trap for future
edits landing in the wrong file.

## Already in good shape

- I-cache and D-cache enabled (libdaisy init)
- Drum synth is fixed-point with a table-free Bhaskara sine approximation
- Voice mixing uses a precomputed 1/√n table (no per-sample sqrt in the live path)
- Stack/bss live in zero-wait-state DTCM (`REGION_ALIAS(RAM, DTCMRAM)`)

## Suggested order (across both repos)

Measure before/after each step with the DWT cycle counter (see the DSP doc's
"Measuring on target" section). Each step is independently shippable.

1. Firmware item 1 (`target-cpu=cortex-m7`) + item 3 (per-crate `opt-level = 3`) —
   config-only, no code changes.
2. DSP P5 (bitcrush / sample-rate-reduce early-outs) and P3 (vocode single sqrt) —
   tiny, zero-risk.
3. DSP P1 (fast-math module + call-site swaps) — the single biggest CPU win; gate on
   its accuracy tests.
4. DSP P4 (oscillator sine LUT) — second biggest for the audio interrupt.
5. DSP P6 (ring-buffer slice ops) then firmware item 2 (BLOCK_SIZE + lock hoisting).
6. DSP P2 (envelope: fast log/exp, then A/B the simple envelope on hardware).
7. Housekeeping (7, 7a, 7b) whenever convenient; items 4/5 (bootloader, ITCM) only if
   profiling still shows pressure after all of the above.
