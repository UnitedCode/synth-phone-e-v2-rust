# MIDI master volume

CC7 covers the same 0–20 master-volume range as the encoder:

| CC7 | Phone level | Master gain |
| --- | --- | --- |
| 0 | 0 | Mute |
| 64 | 10 | Unity (1×) |
| 127 | 20 | Maximum (2×) |

Intermediate values scale linearly to the phone's integer steps. Values above
127 supplied directly to the setter clamp to maximum. MIDI updates redraw the
display and use the same saved master setting as encoder changes.

Previously CC7 stopped at level 10. Existing MIDI sequences sending 127 now
reach the encoder's maximum gain; use 64 for unity. The final output limiter
still applies to the combined mix.
