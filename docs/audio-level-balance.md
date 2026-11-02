# Experimental voice/note balance (`clankers`)

This change is for listening and real-board validation. Keep it on `clankers`;
the completed emulator remains on `main`. Run checks locally, with CI off.

## Problem and change

The main-branch Dry/keypad carrier was mixed at 0.04 inside the DSP, while raw
MIDI notes had a final 0.1 gain plus waveform compensation. Both were quieter
than a nominal vocal input. In the existing volume-settings firmware
`c9d1a58`, Voice Volume scales the *whole* DSP output, including the keypad
carrier; Melody Volume scales only raw melodic MIDI. Turning voice down therefore
turns the keypad down with it and cannot adjust that source ratio. MIDI already
has an independent control in that firmware: its entire reported board problem
is not established by this code finding alone.

The experimental output stage increases generated-tone gain, calibrates the
four waveforms by RMS, and provides independent source controls:

| Control | Range | Action |
| --- | --- | --- |
| Voice Level (CC44) | 0..20; default 10 | Gain on digitized microphone samples **before** vocal DSP; does not scale generated keypad or raw MIDI notes |
| Synth Level (CC45) | 0..20; default 10 | Gain on Dry keypad carrier and melodic MIDI notes; does not scale the microphone or drums |
| Volume | Existing master control | Common gain, followed by the final 0.8 sample ceiling |

Source settings use 0=mute, 1=0.1x, 10=1x, 20=2x. Thus a nonzero source spans
26.02 dB. CC values map by `floor(value*20/127)`: 7->1, 32->5, 51->8,
64->10, 127->20. The new controls appear after Pad Matrix in the encoder menu.
These main-based controls are RAM settings and reset to 10 at boot. This change
does not import the separate feature branch's persistence or mode-selection work.

Keypad calibration compensates the pinned Dry carrier mix (0.04), Hann overlap
sum (2), and final DSP output gain (0.75). Nominal single-note sine output peak
is 0.225; triangle/saw gain is multiplied by sqrt(1.5), square by 1/sqrt(2).
At a 0.2-peak sine vocal reference, each generated source is within 0.05 dB RMS
of the processed vocal. This is a calibration reference, not universal perceived
loudness matching. MIDI velocity and existing shared polyphony scaling remain.
Percussion retains its previous final gain. The vocoder's excitation carrier
is not boosted by this Dry-keypad calibration.

A sample ceiling prevents out-of-range output but can still distort loud sums.
Use source trims/master to retain headroom. Software trim cannot undo analog
preamp/ADC clipping: that requires reducing gain before the clipping stage.

## Real vocal evidence

The authorized local source is the separated Clone Hero vocal stem
`chart-engine/.runtime/head-on-a-plate-stems/vocals.wav`. Use seconds 30..40,
left channel only, resampled from 44100 to 48014 Hz. The listening comparisons
retain their original output levels; there is no per-file normalization.

`audio-lab levels-recording INPUT.wav OUTPUT_DIRECTORY` produces shared-source
DSP/mixer renders, including the frozen main baseline, the frozen existing
settings mix, and the proposed path. It reports RMS/peak and writes isolated
voice/keypad/MIDI, combined sources and comparison WAVs. This command **does not
boot the firmware**. `before-settings-adjusted` reproduces Voice=5/Synth=20:
the legacy keypad/voice ratio stays unchanged. `after-adjusted` exercises those
same settings with the new independent paths. `after-voice-pad` demonstrates
reducing the microphone while retaining a balanced source relationship.

## Hot-preamp test on the virtual embedded device

First build/save both firmware ELFs. Prepare the emulator using the fixed ELF
with the documented `VIRTUAL_DAISY_ELF` environment variable and `startup` command
from [virtual-daisy.md](virtual-daisy.md). Reuse the existing native/.NET caches.
Then run (replace paths; quote those containing spaces):

```text
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml --target HOST_TARGET -- levels RENODE_RUNTIME PREPARED_DIRECTORY BEFORE.elf AFTER.elf VOCAL_48014.wav OUTPUT_DIRECTORY
```

`HOST_TARGET` is the installed native Rust host triple. Set
`VIRTUAL_DAISY_DOTNET` if `dotnet` is not on PATH. Arguments use native paths;
the runner contains no shell scripts or Python. This specific fixture requires
a prepared Rev5/WM8731 serial-PCM model; other board/codec profiles retain their
existing separate emulator tests.

The Rust driver takes one second of the real recording, then simulates preamp
gain up to 0.95 peak (hot, unclipped), and 2.0 peak followed by explicit ADC hard
saturation (overload). It records the gain and count of clipped input frames.
These are specified test conditions, not a measured analog preamp transfer curve.
The prepared emulator receives the resulting stereo ADC PCM through its normal
SAI/codec boundary. Actual ARM firmware boots, services DMA and renders output.

Volume changes arrive as MIDI bytes at USART1; keypad presses connect the actual
matrix GPIO pins. No guest instructions, RAM, settings or DSP outputs are patched.
The optional stimulus CSV consumed by the mandatory C# Renode host contains
`step,uart,usart1,byte`, `step,gpio,port,pin,level`, or matrix
`step,link|unlink,column_port,column_pin,row_port,row_pin`. A step is 10 ms.

The matrix captures baseline voice/keypad/MIDI, fixed hot voice with unity/half
voice gain, fixed keypad and MIDI at settings 1/20 with the hot microphone muted,
and combined hot/overloaded voice plus MIDI. Acceptance requires normal emulator
exit, startup/guard evidence, zero DMA FIFO underruns/overruns, complete stereo
captures, finite bounded samples, at least 3 dB of processed-vocal attenuation
with half input trim (the musical processor is nonlinear),
and both note controls spanning below and above the padded hot voice. Each note's
measured range must be 26.02 dB within 0.25 dB. Metrics use seconds 0.65..0.95;
captures retain the derived SAI sample rate instead of relabeling to 48014 Hz.

The matrix runs at most three captures concurrently and permits 600 wall-clock
seconds per process; a timeout or crash is a failure. Other emulator modes retain
their 180-second watchdog. Wall time is unrelated to emulated audio timing.
`preamp.csv`, `firmware.txt`, per-case stimuli, RTT/CPU/DMA/codec logs, WAVs,
`levels.csv`, and `result.txt` retain the evidence. A stale `result.txt` is removed
before a new run, and firmware hashes must remain unchanged throughout capture.
`levels-analyze OUTPUT_DIRECTORY` rechecks saved audio and boot/stream evidence;
it does not independently establish successful emulator process exit. Exact
linear input trim is covered by the shared-source tone regression. Do not require
an exact 6.02 dB processed-output change on human vocals: thresholds, pitch
tracking and frame history can change the resulting audio as input level changes.

Real-board verification is still required for the physical preamp's gain and
clipping curve, codec tolerances, converter response and measured timing. The
fixture establishes digital routing, control range and functional firmware output.

## Measured virtual-device result

The before ELF is the unchanged settings firmware `c9d1a58`; after ELF SHA256
is `bd54d11fa0932af35ab51dbe73594f8550562ad80c74f483e56f6a03e1066828`.
The one-second excerpt needs 1.3656069x preamp gain to reach 0.95 peak without
ADC clipping; 2.874962x reaches 2.0 peak and clips 1,285 of 48,014 input frames.

| Measurement (RMS window) | Result |
| --- | --- |
| Existing keypad vs hot voice | -9.4 dB |
| Existing MIDI vs hot voice | -5.9 dB |
| Fixed Voice Level 10 -> 5 | -7.25 dB processed output |
| Fixed keypad vs padded voice, Synth 1 -> 20 | -9.5 -> +16.6 dB |
| Fixed MIDI vs padded voice, Synth 1 -> 20 | -9.4 -> +16.6 dB |

All 11 emulator child processes completed normally, with 48,099 output frames
each, zero FIFO underruns/overruns, and matching stereo output. The fixed hot
voice and hot voice/MIDI mix had no samples at the output ceiling. The original
hot voice hit the ceiling on 42 frames; the intentionally overloaded fixed mix
hit it on 60 frames. The overload example is not a clean-audio success claim.

The first analyzer incorrectly required an exact 6.02 dB **processed-output**
change for half **input** gain. It rejected the measured 7.25 dB attenuation after
all 11 captures completed. The analyzer now checks useful attenuation and note
range across the nonlinear musical processor, while the shared-source tone test
checks the exact gain behavior. Saved captures were reanalyzed without changing
the firmware. Replacing the captured high keypad note with its low-level capture
was rejected by the overlap gate; incomplete evidence also removes stale passes.

Verification: 43 audio-lab tests on macOS and Linux; virtual-Daisy native tests
on macOS; Windows x64 and Linux ARM64 checks of all updated tool targets; ARM
firmware build and Rev5 startup. Windows native execution and physical hardware
listening remain unverified. No GitHub Actions runs are needed for these checks.
