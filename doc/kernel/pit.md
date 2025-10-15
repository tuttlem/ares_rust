# Programmable Interval Timer (PIT)

File: `src/arch/x86_64/kernel/pit.rs`.

## Configuration

- Uses the legacy PIT clock (1,193,182 Hz) to generate periodic interrupts.
- `init_frequency(hz)` programs channel 0 in mode 3 (square wave) by writing to ports 0x43 and 0x40.
- Divisor values are clamped to `[1, 65535]` to stay within hardware limits.

## Usage

Called from `timer::init()` to set a 100 Hz tick rate. The PIT interrupt is mapped to vector 32 after PIC remapping and drives the scheduler’s heartbeat.

If you change the tick rate:

1. Update constants in `timer.rs` if tighter slice timings are required.
2. Consider scaling log output to avoid overwhelming the serial port at higher frequencies.
