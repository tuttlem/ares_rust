# PS/2 Keyboard Driver

Source: `src/arch/x86_64/drivers/keyboard.rs` with the portable façade in `src/kernel/drivers/keyboard.rs`.

## Responsibilities

- Initialises the PS/2 controller (enables scanning, clears residual bytes).
- Translates set-1 scancodes into ASCII bytes.
- Buffers input in a fixed-size ring until userspace (the `init` shell) reads from file descriptor 0.
- Signals waiting processes via the driver registry when new data arrives.

## Architecture layer

`keyboard.rs` exposes:

- `init()` – programs the controller, flushes the output buffer, and enables IRQ1.
- `handle_interrupt()` – called from the IRQ handler, decodes scancodes, applies modifier state (Shift, Ctrl), and pushes bytes into the buffer if there is space.
- `read(buf)` – pops bytes from the ring into the provided mutable slice.

A `SpinLock<KeyboardState>` ensures interrupt handlers and consumer reads coordinate around the buffer indices.

## Portable layer

`kernel/drivers/keyboard.rs` implements the `CharDevice` trait. Reads block on a process wait-channel if the buffer is empty:

1. Attempt a non-blocking read via the architecture helper.
2. If nothing is available, the caller is blocked on `WaitChannel::KeyboardInput` and the scheduler is invoked.
3. The IRQ path wakes waiting processes when new bytes arrive.

## Notes

- Only ASCII output is currently produced (no Unicode translation table).
- Key release scancodes are ignored—only press events are exposed.
- The buffer size (256 bytes) and modifier behaviour should be kept in sync with any future console enhancements (e.g., command history).
