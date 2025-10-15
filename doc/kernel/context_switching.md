# Context Switching

Sources:
- Assembly: `src/arch/x86_64/kernel/context_switch.asm`
- Scheduler logic: `src/kernel/process/mod.rs`

## Saved context (`Context`)

Each process owns a `Context` struct containing callee-saved registers, `rsp`, `rip`, and `rflags`. During a switch:

1. The current context pointer is passed to `context_switch` as a mutable pointer.
2. The function saves `r15`..`rbx`, `rbp`, `rsp`, a synthetic return address, and flags.
3. The next context pointer is loaded, registers/stack are restored, and control jumps to the recorded `rip`.

## Cooperative switches

When a task calls `process::yield_now()` or blocks (e.g., waiting on a child), the scheduler:

1. Locks the process table, marks the current process as `Ready` or `Blocked`.
2. Picks the next `Ready` process (`next_ready_index`) preferring non-idle tasks.
3. Updates `CURRENT_PID`, increments the destination’s `cpu_slices`, and invokes `context_switch`.

## Preemption

Timer interrupts piggyback on the same mechanism:

1. `request_preempt` stores the interrupted `rip` in `process.preempt_return` and patches the trap frame to jump into `preempt_trampoline` upon return.
2. `preempt_do_switch` clears `NEED_RESCHED`, calls `reschedule()`, and finally returns the saved `preempt_return` (or `context.rip` if none was recorded).
3. The assembly trampoline (`preempt_trampoline`) simply calls `preempt_do_switch` and jumps to the returned RIP.

## Process exit

`process::exit_current()` marks the current process as a zombie, wakes the parent if waiting, and calls `reschedule()` before spinning. The zombie is removed when the parent reaps it (`wait_for_child`).

## Notes

- Kernel stacks are 16 KiB, 16-byte aligned, with `process_exit` placed at the top as a guard.
- Idle process – PID 1 loops on `hlt` and is used only when no other task is runnable.
- Statistics – `cpu_slices` tracks how many time slices each process has consumed; `scheduler_stats()` reports aggregate counts for diagnostics.
