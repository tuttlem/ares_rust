# Process Management & Scheduler

Primary source: `src/kernel/process/mod.rs`.

## Data structures

- **Process** – Represents a kernel task. Fields include PID, parent PID, state (`Ready`, `Running`, `Blocked`, `Zombie`), wait channel, exit code, idle flag, saved context, kernel stack pointer/layout, open file descriptors, tracked memory regions, and a preemption return slot.
- **ProcessTable** – Backed by a dynamically growable array allocated on the kernel heap. Protected by a `SpinLock`.
- **MemoryRegionList** – Tracks per-process heap/stack allocations to support diagnostics and eventual teardown.

## Lifecycle

1. `process::init()` creates the idle task and marks the table initialised.
2. `spawn_kernel_process(name, entry)` allocates a stack, seeds the context to start at `entry`, and initialises the default file descriptor table (keyboard → stdin, console → stdout/stderr).
3. The parent PID is recorded so exit codes can be reaped via `wait_for_child`.

## Scheduling

- `schedule_internal()` finds the next runnable process in a round-robin fashion (preferring non-idle tasks). It updates process states and performs the context switch.
- `yield_now()` / `reschedule()` wrap the scheduler for cooperative switching.
- `NEED_RESCHED` indicates a pending preemption request to avoid redundant work.

## Blocking & waking

- `block_current(channel)` transitions the current process to `Blocked`, records the wait channel, and reschedules.
- `wake_channel(event)` scans blocked processes and wakes any whose wait channel matches the event (keyboard input, child exit, etc.).

## Exit & zombies

- `exit_current(code)` marks the process as a zombie, stores the exit code, and wakes the parent.
- `wait_for_child(target)` blocks until the specified child (or any child) exits, then removes the zombie from the table and returns its exit status.

## Diagnostics

- `dump_process(pid)` / `dump_all_processes()` log registers, stack pointers, descriptor tables, memory regions, and scheduler stats to aid debugging.
- Scheduler stats include totals for each state, overall slice counts, and whether a reschedule is pending.

## File descriptors

- Up to 16 descriptors per process (`MAX_FDS`).
- Entries wrap `FileDescriptor::Char`, pointing at devices registered via `drivers::register`.
- Accessed during syscalls through `process::descriptor(pid, fd)`.

## Next steps / ideas

- Implement `free_frame` and per-process heap reclamation to support long-running workloads.
- Add priority scheduling or different policies if cooperative fairness becomes an issue.
- Introduce user-space isolation (separate address spaces) once the MMU layer is expanded.
