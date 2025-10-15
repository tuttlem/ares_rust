# System Call Layer

Sources:
- Architecture: `src/arch/x86_64/kernel/syscall.rs`
- Portable facade: `src/kernel/syscall/mod.rs`
- Entry stubs: `src/arch/x86_64/kernel/syscall_entry.asm`

## Initialisation

`syscall::init()` programs:

- `IA32_EFER` – sets the SCE bit to enable `syscall/sysret`.
- `IA32_STAR` – configures the kernel/user code selectors used during transitions.
- `IA32_LSTAR` – points to the `syscall_entry` assembly stub.
- `IA32_FMASK` – masks TF/IF so the kernel enters with interrupts disabled and the trap flag cleared.

## Fast path

1. `syscall_entry` saves a subset of registers and calls the Rust trampoline with a pointer to `SyscallFrame`.
2. `syscall_trampoline(frame)` invokes `dispatch(frame)` which switches on `frame.rax` (the syscall number).
3. Supported syscalls: `read`, `write`, `yield`, `exit` (following Linux numbering conventions).

## Dispatch flow

- The dispatcher resolves the current PID, fetches the file descriptor from the process table (`process::descriptor`), and delegates to the `CharDevice` implementation.
- Errors return sentinel `u64::MAX - n` values (`ERR_BADF`, `ERR_FAULT`, `ERR_NOSYS`).
- `sys_yield()` calls `process::yield_now()` to voluntarily hand the CPU to the scheduler.
- `sys_exit(status)` calls `process::exit_current(status)`, marking the process as a zombie and waking the parent.

## Kernel-internal helpers

The module also exposes `write`, `read`, `yield_now`, and `exit` wrappers that construct a `SyscallFrame` and reuse the dispatcher. This allows in-kernel tasks to exercise the same code paths as user tasks.

## Extending the ABI

- Add new numbers to the `nr` module and extend `dispatch` with the desired behaviour.
- Validate pointers and lengths carefully; the current ABI assumes kernel tasks are well-behaved and shares address space with them.
- Update documentation here to record semantics and return codes.
