# Kernel Timer

File: `src/arch/x86_64/kernel/timer.rs` (architecture layer) and `src/kernel/timer` (portable facade).

## Responsibilities

- Initialise the PIT at the requested frequency (default 100 Hz).
- Maintain a global `TICK_COUNT` (`AtomicU64`).
- Request scheduler preemption on a fixed cadence.

## Flow

1. `timer::init()` stores the PIT frequency, registers `timer_handler` for vector 32, enables the IRQ line, and programs the PIT via `pit::init_frequency`.
2. `timer_handler(frame)` increments the tick counter and, when `tick % PREEMPT_SLICE_TICKS == 0`, calls `process::request_preempt(frame)`.
3. `ticks()` exposes the ticking counter to other subsystems (e.g., the ticker demo tasks).

The current preemption slice is 1 tick (i.e., the handler requests a context switch every interrupt). Adjust `PREEMPT_SLICE_TICKS` if you need coarser slices.

Keep work inside the interrupt handler minimal—long operations should be deferred to scheduled tasks.
