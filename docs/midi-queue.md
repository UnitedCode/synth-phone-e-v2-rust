# MIDI queue overload

UART MIDI and keypad notes share a 127-event queue. Normal traffic stays FIFO.
When it fills:

- Unhandled events are rejected without changing the queue.
- A useful incoming event can replace an old ordinary CC, pitch bend, or
  unhandled event. Retained events keep their original order.
- Note releases, zero-velocity NoteOns, sustain release (CC64 below 64), and
  CC120/121/123 take priority over new notes, program changes, and pedal-down.
  Queued sustain and panic controls are protected from ordinary traffic.
- If the entire queue contains releases or recovery markers, another release
  replaces that backlog with an emergency reset followed by the incoming event.
  The audio task stops all voices and clears sustain and pitch bend on every
  channel before processing later events. This can cut off held notes during
  extreme overload; it prevents lost releases from leaving notes stuck.

The overflow path uses no allocation or temporary event array. It examines at
most the fixed queue capacity per scan and rotates the complete queue once
when removing an event. Producers hold the existing RTIC queue lock throughout.

Queue occupancy is checked every 4,800 processed audio samples (roughly 100 ms),
independently of FFT hop resets. Occupancy above 75% produces at most one warning
per interval. This periodic snapshot can miss shorter peaks; overflow recovery
still runs for each enqueue attempt.

UART framing, parity and overrun errors enqueue a protected recovery marker.
The audio task stops MIDI voices (including held phone sounds) and clears sustain
and pitch bend before later notes. Pending receive bytes are discarded with a
32-read budget per call, then parsing waits for a fresh status byte. Recovery
can cut off held sounds because their releases may have been lost. Noise-only
flags preserve the byte stream and running status.

MIDI System Reset (`0xFF`) is a protected queue event. It stops MIDI voices,
clears sustain/pitch bend and transient lead routing, restores the default
triangle waveform and MIDI harmony levels (lead 127, backing 64), and redraws
the display. The receiver also clears running status and partial messages;
following messages are parsed normally without flushing the UART. Other real-time
bytes remain transparent to the parser. Saved phone volume/menu settings are
preserved. A pending System Reset survives later critical queue overflow.

The [MIDI 1.0 specification, System Reset section](https://www.freqsound.com/SIRA/MIDI%20Specification.pdf)
defines voice/controller reset and clearing running status. This phone has no
MIDI sequencer or configurable Omni/Local modes to reset.

`tools/audio-lab/tests/midi_queue.rs` exercises the firmware queue implementation
with the real `heapless` queue, including all removal slots and ring positions,
rejection without mutation, pedal/panic protection, and VoiceManager recovery.
Host tests do not establish interrupt timing on the physical phone.
