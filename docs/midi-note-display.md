# MIDI note display

The processing screen shows the newest active pitched MIDI note. Releasing it
restores the previous active MIDI note, or the held keypad note when MIDI is
silent. Sustain keeps released notes visible until pedal-up. Panic controls and
queue-overflow recovery clear the same voices that drive the label.

- All pitched MIDI channels participate, including silent vocal-control voices.
- Channel 10 drums, ringers and dial tones never supply a pitch label.
- Voice stealing and rejected notes follow the actual voice slots.
- MIDI labels use absolute pitch classes (`C`, `C#`, … `B`), independent of the
  selected keypad key. Octaves and pitch bend do not change these labels.
- Keypad note tracking and its audio/DSP behavior stay independent of the label.
- Percussion keeps its sound-set label; effects and menu screens keep their
  existing layouts. MIDI state remains current while those screens are open.

The audio task updates the display state after applying its MIDI batch and
coalesces redraw requests once per audio block. No display I/O runs there.
Regression tests use the firmware VoiceManager, state machine and note font;
physical OLED response and interrupt timing still require phone validation.
