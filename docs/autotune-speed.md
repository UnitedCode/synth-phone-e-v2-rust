# Autotune speed and streaming correction on clankers

The previous firmware displayed and saved Autotune Speed but never used it to
configure vocal processing. The DSP applied a fixed 0.99 correction coefficient
from a previous ratio of 1.0 on every frame. There was no retained correction
trajectory, so a slower transition could not accumulate toward its target.

The firmware now uses the DSP's `process_vocal_effects_1024_with_state` entry
point, pinned at `cddeddc84ad5b1d11a8c7e15f414f381186e633f` on the separate
`codex/clankers-autotune` DSP branch. Firmware, host replay, ARM guest and native
emulator tools share that exact revision. Neither repository's main is changed.
The legacy by-value DSP APIs remain compatible for other callers.

## Controls and state

Autotune Speed configures the fraction of remaining correction-ratio error
applied each FFT hop. Higher values are faster:

| Menu value | Per-hop fraction | Approximate correction time constant |
| --- | --- | --- |
| 1 | 0.01 | 530 ms |
| 5 | 0.10 | 51 ms |
| 10 | 1.00 | One hop, immediate ratio update |

The intermediate coefficients are 0.018, 0.032, 0.056, 0.18, 0.32, 0.56 and
0.8. These times describe the correction controller, excluding FFT windows,
overlap-add and I/O latency. They are not calibrated end-to-end board timings.

The fresh menu default is now **10**, preserving the old near-instantaneous
sound. Existing saved values are retained: a saved 5 now produces a moderate
glide instead of acting like every other value. Menu editing and MIDI CC46
both update the same setting. CC46 maps 0..127 into menu values 1..10; it is new
and is ignored by the old firmware. The settings record layout/count is unchanged.

A single FFT-local ratio persists across hops. Zero-energy or invalid-frequency
input releases it to unity; switching to Dry, Vocode or Harmony also releases it.
A new voiced phrase therefore does not inherit a stale correction from an
earlier phrase. Speed changes keep the current ratio and affect subsequent hops.
The unused duplicate shared ratio has been removed. Host replay and the ARM
guest retain/reset state through the same public entry point.

The stateful API also honors correction strength. Firmware explicitly retains
the old 99% steady-state correction amount; transition speed changes how it
gets there. The fast coefficient returns that target directly, avoiding an
unnecessary subtract/add round trip. The pitch estimator and spectral/formant
algorithms are unchanged.

## Verification

PCM tests cover a warm 35-cent note transition, fast capture versus slow glide,
eventual slow settling, release across a silent gap, encoder-menu editing,
CC46 and saved-setting restoration. Replacing retained state with unity every
frame fails the slow-settling test at about 34.66 cents from target. The old
by-value processing path also fails the slow-glide assertion.

The 720-case steady-pitch sweep uses the preserved fast default. It retains
528 passes / 192 known failures, with zero new failures or improvements. Both
speed and strength probes now respond. The speed probe compares the fast
default with 0.01, rather than comparing the default with itself. No failure
IDs or acceptance tolerances were relaxed or added to the baseline.

The known pitch/formant quality limitations remain. The final medium preset (5)
sweep reports 531 passes / 189 failures: 9 newly flagged female-formant cases and
12 improvements against the legacy fast baseline. It exits unsuccessfully;
this is not a regression-free result for every speed/formant combination.
The existing autocorrelation detector also has octave ambiguities on strongly
reshaped harmonics, so these need listening/spectral investigation. The attempted
detector and spectral changes were excluded from this focused fix. Keep the
failed reports as evidence; no failure IDs were added to hide these flags.

Reproduce a selected preset with `audio-lab sweep-speed OUTPUT_DIRECTORY 5`.
It retains the original 0.65-second fixtures and 0.2-second measurement warmup;
at very slow settings that window can include the intentional glide. Use the
separate settling test to distinguish slow response from a missed target.

Local checks cover 82 audio/source tests and 210 emulator/tool tests on macOS
and Linux, the DSP's 45 existing tests, a no-std ARM build and Windows GNU
all-target compilation. Windows runtime and real-board acceptance remain
separate checks. Use local checks and `[skip ci]`; no CI run is needed.

## Real-vocal firmware captures

Prepare Rev5/WM8731 serial PCM using `startup` with `VIRTUAL_DAISY_ELF` pointing
to the built branch ELF, then run:

```text
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml --target HOST_TARGET -- autotune RENODE_RUNTIME PREPARED_DIRECTORY BEFORE.elf AFTER.elf VOCAL_48014.wav OUTPUT_DIRECTORY
```

`HOST_TARGET` is the installed host triple. `VIRTUAL_DAISY_DOTNET` can select
the local .NET runtime. Paths are passed as structured command arguments on
Windows, Linux and macOS. The driver uses at most three captures concurrently,
with a 600-second wall-clock bound per capture and no guest code/RAM patching.

Five four-second runs use the same authorized Clone Hero separated vocal
excerpt, first 4.1 seconds, with one fixed input gain giving 0.35 peak. Voice
Level is 10, Synth Level is 0, and formants are off. UART sends CC46 requests
for old slow/fast reference captures and new slow/medium/fast captures. The old
image ignores CC46; its two recordings establish a repeatable legacy fast
reference, not successful setting changes on that image. Menu wiring and the
old DSP's ignored speed are independently covered by the PCM/control tests.

Capture grading requires successful process exits and matching firmware hashes,
full boot and settings-save evidence, finite identical stereo, no output-ceiling
hits, expected frame counts from actual SAI clocks, and zero DMA underruns or
overruns. Comparisons use seconds 0.6..3.8, with at most 64 samples of constant
alignment and no gain correction or time warping. The legacy reference must
match; the three new presets must produce distinct PCM.

`autotune-analyze OUTPUT_DIRECTORY` rechecks saved captures. Missing/mismatched
images, incomplete runs and an old PASS marker cannot substitute for new
evidence. PCM differences establish control response, not intended-note accuracy
or perceived musical quality. Listen to the before/after files and verify on a
real board before merging this experiment into main.

The 2026-11-02 capture set passed all five cases. Both legacy WAVs are
byte-identical. Updated slow/medium/fast PCM is distinct; all runs saved
settings and have zero DMA underruns/overruns, no output-ceiling hits, and no
FFT-spawn or dropped-MIDI warnings. Each capture contains 192662 stereo frames.
The baseline ELF SHA-256 is
`8c136ae6abd352d717ab009c0a35232a0b1549f1d514b50865ae56b42b8baf9b`;
the tested branch ELF is
`624bed8e3b033a46ba41f3c3599baa170be436464db1ec0dcfaaa45b899ef4fe`.

Substituting slow PCM for the fast capture fails the response gate. Substituting
a different firmware hash also fails. Both negative checks remove an old PASS.
Listening pairs trim the same first 0.6 seconds, retain captured gain/rate, put
before first and after second with a 0.4-second silent gap, and use PCM16 WAV.
The original float captures and metadata remain available with the evidence.

## Audibility follow-up

The user could not distinguish the original listening examples. The PCM-distance
gate establishes a changed signal, not an audible retuning benefit. A 220 Hz
sine/cosine phase control gives relative PCM difference 1.4142 with only 0.0018
cents of measured pitch difference. In the original recording, the 38 comparable
120 ms windows had a median slow/fast difference of 10.57 cents; another 18
paired windows were too ambiguous to interpret. These overlapping windows are
diagnostics, not independent samples or a musical quality score.

`examples/inspect_retune.rs` reproduces the pitch audit and phase control,
scans float vocal WAVs for sustained sections, and checks the new two-second
native capture set. Use `--example inspect_retune -- phase-control`,
`--example inspect_retune -- CAPTURE_DIRECTORY 0.6`, or
`--example inspect_retune -- native VOWEL_CAPTURE_DIRECTORY` with the normal
audio-lab Cargo command. Its `host` mode is explicitly shared-DSP preflight,
not firmware emulation.

A second test uses a held vowel from the same original recording at seconds
69.0657..69.9855. Before entering the emulator it is raised about 80 cents,
scaled by 0.35, preceded by 400 ms of silence and padded to 2.1 seconds. The
exact same stimulus is used for old, new-fast and new-slow production images.
All three two-second firmware captures pass CPU/boot, stereo, frame/clock,
ceiling and DMA checks. This run is too short to test settings persistence.
The images are unchanged from the earlier test.

The controlled vowel yields 65.95 cents median slow/fast difference across 27
comparable windows, with two ambiguous windows. The before/slow comparison is
68.49 cents across 26 comparable windows. The new listening file repeats
before/slow twice, with identical 4x playback gain on every section. There is
no output pitch shifting, dynamic compression or per-section normalization;
raw captures remain available. This is a controlled input test, not an
unaltered performance or proof that ordinary singing sounds better. Listening
acceptance remains open; a clean, natural retune trajectory is not established
by PCM differences or these pitch-window summaries alone.
