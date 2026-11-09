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

`tools/audio-lab/tests/midi_queue.rs` exercises the firmware queue implementation
with the real `heapless` queue, including all removal slots and ring positions,
rejection without mutation, pedal/panic protection, and VoiceManager recovery.
Host tests do not establish interrupt timing on the physical phone.
