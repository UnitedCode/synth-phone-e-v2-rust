# MIDI settings and octave-fix integration

This merges firmware `feat-midi-settings` (`e21a19f`) into main and pins the vocal
DSP to `59a1c1a`, from `fix/scale-octave-wrap`. The DSP branch is merged into its
own repository's main separately. Firmware, host replay, ARM guests and emulator
lockfiles all use the same exact DSP revision.

The firmware adds Tuned mode (scale-corrected voice with a separately played
keypad synth), a separate Synth Octave setting, mode enable switches, ringers,
DTMF sounds and persistent master/menu settings. MIDI channel roles are:

| Human MIDI channel | Role |
| --- | --- |
| 1 | Vocal-effect pitch control, no raw synth audio |
| 2 | Synth audio without retuning the voice |
| 3 | Synth audio normally; vocal-effect control without raw synth in Vocode/Harmony |
| 10 | GM drums 35..81, held ringers 82..93 and held DTMF sounds 94..105 |

The existing branch's 6x melody makeup gain is preserved. The experimental
`clankers` source-balance fix is not part of this merge: Voice Volume still
scales the processed output, including a keypad carrier mixed into that output.
Hardware balance validation of `clankers` must be repeated against this baseline.

Saved settings use the branch's v2 record: master plus every `MENU_ITEMS` value,
CRC protection and alternating 4 KiB QSPI sectors at 0x7FC000/0x7FD000. Old v1
or incompatible-menu-count records are ignored. Restoration remains deferred
until after init constructs the large audio buffers in place.

Integration review corrected two persistence hazards. Flash load/save now runs
in a priority-1 software task, while keypad scanning stays at priority 3 and MIDI
receive runs at priority 4. Long erase/program polling therefore cannot hold up
the input scan or MIDI receiver. Audio/FFT retain priorities 8/7. Changes are
compared across interface ticks, so MIDI-originated settings updates also start
the save countdown; failed or queued saves retry after another quiet interval.

The handler merge retains main's shared `vocal_frame::process_frame` entry point
and the feature branch's Tuned carrier layer. The original MIDI frequency table
was moved unchanged into a HAL-free module so tests use the actual voice manager.
The host lab also compiles the actual state machine, storage settings types,
percussion mapper and sound engines, avoiding the hardware HAL in desktop tests.

## Local verification, with CI skipped

- DSP: 45 unit tests and a no-std Cortex-M7 build check.
- Host audio/features: 69 tests on macOS and Linux, including all 47 GM drums,
  held release of all 24 phone sounds, MIDI routing, saved-setting restoration
  and clamping, mode selection, and octave regression tests.
- The octave tests fail against the old DSP and pass against the new revision.
- The 720-case pitch sweep has zero new failures against its recorded baseline.
  It still has 192 known quality failures and two unresponsive correction controls.
  Large combined pitch shifts also retain quality limitations; this is not a
  claim that the entire pitch processor is correct.
- Cortex-M firmware release build and virtual Rev5 startup pass.
- Windows cross-target checks cover the updated host tools. Native Windows
  execution and physical device acceptance remain separate checks.

The new firmware also exposed a native emulator retry bug: an LDRD whose base
register aliases its first destination could overwrite that base before a
second-word stall restarted the instruction. The pinned ARM decoder adapter now
loads both words into temporaries before committing either destination. A real
ARM flash-boundary guest fails without the fix and passes with it; the existing
CPU/FPU/fault, SAI, SDRAM and bus/exception probes remain green.

Run the settings integration scenario after configuring the documented local
Renode/.NET runtime and `VIRTUAL_DAISY_ELF`:

```text
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml --target HOST_TARGET -- settings RENODE_RUNTIME OUTPUT_DIRECTORY --board rev5
```

This runs three virtual seconds. Real UART bytes change master volume, then send
NoteOn/NoteOff during/after the nominal save interval. The test requires a
successful firmware save message, audible note-on output, silent note-off output,
finite matching stereo, and zero FIFO underruns/overruns. The observed note-on
RMS is 0.206 and the final note-off RMS is zero. Each process has a bounded
600-second wall-clock watchdog for this longer capture; other modes retain 180s.
The bridge's optional `VIRTUAL_DAISY_AUDIO_STEPS` accepts 1..1000 ten-ms steps,
defaulting to the original 100. Stimulus CSV only dispatches physical UART/GPIO
inputs; it never patches guest instructions or settings RAM.

Physical flash retention over power loss, analog sound quality and calibrated
silicon timing still require a real board. No new CI workflow or paid run is
required for these local checks.
