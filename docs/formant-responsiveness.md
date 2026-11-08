# Formant responsiveness on clankers

The measurements below describe the responsiveness change at its original DSP
pin. The later [pitch audit](pitch-audit.md) updates correction/formant processing
and supersedes the historical 192-case quality-failure count.

[Issue 98](https://github.com/UnitedCode/synth-phone-e-v2-rust/issues/98)
reports slow screen refresh and missed keypad presses when formant is enabled,
including difficulty switching it off. This change reduces the formant work and
prevents a complete FFT job from blocking the input handlers. Physical-board
acceptance remains required: the preceding firmware already passed the model's
20 ms keypress fixtures, so those fixtures do not reproduce the reported loss.

## Changes

The cepstral envelope previously used a full complex inverse FFT followed by a
real forward FFT on every active-formant hop. Only the real part of the inverse
was used. For real input, `Re(IFFT(x))[k] = Re(FFT(x))[k] / N`; the real part is
even in k. Two real FFTs therefore compute the same envelope. The existing
endpoint convention, lifter, log/exp functions, formant ratios, and per-hop
refresh cadence are retained. The 1024-point envelope scratch arrays shrink
from 12 KiB to 8 KiB. Independent double-precision direct-DFT tests cover all
four supported FFT sizes, silence, isolated spectral lines, smooth envelopes
and magnitudes spanning several orders of magnitude, with a 0.0025% relative
error bound. An operation-budget test rejects use of a complex inverse FFT.

The RTIC priorities now put short input IRQs above the FFT task:

| Task | Priority |
| --- | ---: |
| Audio DMA | 8 |
| MIDI USART1 | 7 |
| TIM2 keypad/encoder scan | 6 |
| Vocal FFT | 5 |
| Display | 2 |
| Startup / settings persistence | 1 |

The scanner can preempt formant processing, and MIDI can preempt both. Existing
short shared-resource locks retain RTIC's priority-ceiling protection. Audio DMA
remains highest. Matrix settling delays and physical switch handling are not
changed. The display still renders in the background; the model does not
measure actual OLED refresh latency.

The envelope optimization affects Pitch Control, Tuned (the same pitch path),
and Dry with formant selected. The streaming Harmony envelope, pitch tracking,
voice phases and lead/backing gains are unchanged. Its state remains 8288 bytes
in CPU-only AXI SRAM, and DMA buffers remain in D2 SRAM. The explicit MIDI lead
controls described in [harmony-midi.md](harmony-midi.md) are preserved.

## Local verification and reproduction

DSP pin: `cf493b7a6c8c6b5addef2a0c649dc10795f46287`, based directly on the
previous clankers pin `b2163e5a2bd6c03bd32772c9a571f2454c58fd7b`.

- macOS and Linux audio/MIDI/tool suites: 115 distinct tests, including the
  final targeted formant-fixture tests after the full-suite run.
- Windows GNU all-target cross-compilation; Windows execution is not claimed.
- DSP: 50 unit tests, 4 integration tests and 12 doctests pass.
- QEMU Cortex-M7: fourteen DSP/replay cases match host PCM within 0.0001 peak
  error, including both pitch-formant presets, Dry formant, eight-voice Harmony
  and weighted MIDI lead. Interrupts and deliberately lost-frame detection run.
- The 720-case pitch sweep has zero new failures; the same 192 pre-existing
  pitch-quality failures remain. This change does not fix those failures.
- The full 288-second lead-forward Harmony render is byte-identical to the
  previous revision: SHA256
  `8224f2827fc252f4ee484135cf66fa67083d7d15db3da2c9c74563db29920841`.
  The external arrangement remains a provisional melody reference; it is not
  a verified transcription. Music, recordings and generated audio stay outside Git.
- A host-only, interleaved envelope benchmark measured about 1.09x speedup
  (9.99 us to 9.13 us median per envelope) in one local run. This is neither a
  whole-firmware speedup nor a physical Daisy cycle measurement.
- Seven complete-production-ELF virtual Rev5 captures pass. The three formant
  fixtures recognize all 14, 17 and 13 expected presses; the four existing
  MIDI lead fixtures retain equal/focused/muted/eight-note behavior. All have
  clear CFSR/HFSR, audible finite matching stereo, zero reported dropped tasks
  and DMA FIFO errors, and no unexpected emulator warnings. The lead/backing
  spectral-ratio change is 12.227 dB against the nominal 11.973 dB. The old ELF
  also passes all three button fixtures, so this is regression coverage rather
  than reproduction of the physical bug.

```text
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example formant_capture -- prepare NEW_DIRECTORY
```

The physical-input scripts select formant through real USART1 MIDI bytes, then
close matrix row/column contacts for 20 ms. Pitch Control and Tuned fixtures
exercise repeated male/female/off keys. A third Tuned fixture exercises ordinary
key-up/down buttons while formant is active, followed by formant-off. The
separate `midi_lead_capture` fixtures retain the four complete-firmware
equal/focused/muted/eight-note scenarios. Use the same authorized real-vocal
`input.f32le` from `harmony_capture prepare`, and run each through the production
ELF with `VIRTUAL_DAISY_AUDIO_STEPS=200`.

Preserve the actual ELF SHA256 as `firmware_sha256=HASH` in `firmware.txt` and
in each successful `NAME.completed.txt`, alongside `exit=0`. Record completion
only after the emulator exits successfully. Then run:

```text
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example formant_capture -- inspect DIRECTORY
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example midi_lead_capture -- inspect HARMONY_DIRECTORY
```

The formant audit requires every expected button in order, including repeated
same-key presses, audible finite matching stereo, clear CPU fault registers,
the CPU execution guards, zero reported dropped tasks and DMA FIFO errors,
and no unexpected emulator warnings. It does not measure contact bounce,
physical input latency, display timing or CPU deadline margin. These were local
checks; the physical acceptance work below remains necessary.

For physical acceptance, enable male/female formant, sing while pressing the
usual controls, and switch formant off repeatedly. Check recognition, display
response and audio interruptions, then repeat the existing eight-note Harmony
performance. No new MIDI setup is needed to exercise the formant fix.
