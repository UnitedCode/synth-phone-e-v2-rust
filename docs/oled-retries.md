# OLED write retries

A failed frame write leaves an update pending and starts a 100 ms retry delay.
The existing input timer wakes the background display task when it is due, so
recovery does not require another button press or MIDI event. New requests are
coalesced during the delay; they do not bypass it.

Each attempt clears and redraws the full frame from the latest application
state. This also repairs partial transfers: the pinned SSD1306 driver clears
its dirty region before attempting bus I/O. Requests arriving during a write
remain pending after success. Retries stop after success unless a newer update
is pending.

Rendering and I2C stay in the priority-2 display task, outside shared-state
locks. The retry clock uses the enabled DWT counter and actual core frequency,
with wrapping subtraction across counter rollover.

Host tests use the firmware update controller and the same SSD1306 version as
the device, injecting command/data failures and checking a complete 512-byte
replacement frame. Physical I2C recovery and display timing still need a phone
test.
