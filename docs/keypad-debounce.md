# Keypad debounce

Each of the twelve keypad contacts must keep the same sampled level for at least
5 ms before a press or release is dispatched. A change back during that interval
restarts only that contact's timer. A held key emits one press and one eventual
release; it does not repeat.

The existing 1 ms TIM2 scan and column-settling delays are retained. Debounce uses
the enabled DWT core-cycle counter and the configured core clock, so its interval
does not depend on counting successful UI interrupts. Sampling jitter can delay
delivery until the next scan. Contact activity wholly between scans remains
unobservable. Normal 20 ms taps are covered by deterministic tests; pulses shorter
than 5 ms are intentionally rejected. Stable contacts add about 5–6 ms of input
latency at the normal scan cadence, in addition to scheduling and audio latency.

The filter runs before both musical-keypad and percussion routing. Releases still
stop the note their press started after screen/profile changes, and overlapping
pitched notes keep their last-pressed priority. Key mapping, Pad Matrix, encoder
and hangup-switch behavior are unchanged. The matrix history is owned locally by
the UI task; it no longer needs a shared resource lock for every contact.

Host regression fixtures cover press/release chatter, rejected short pulses,
repeated 20 ms taps, independent and simultaneous contacts, delayed scans, cycle
counter wraparound, actual 480 MHz timing, overlapping notes, menu navigation and
held ringer release. The embedded release build checks the real RTIC wiring.
Physical contact bounce and worst-case scan latency while singing with formants
enabled still require board testing.
