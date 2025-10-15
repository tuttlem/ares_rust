# Model-Specific Register Helpers

File: `src/arch/x86_64/kernel/msr.rs`.

Two inline helpers wrap the `rdmsr`/`wrmsr` instructions:

- `unsafe fn write(msr: u32, value: u64)` – writes a 64-bit value to the given MSR index.
- `unsafe fn read(msr: u32) -> u64` – reads and returns the 64-bit value.

The syscall subsystem uses these helpers to configure:

- `IA32_EFER` – enable `syscall/sysret`.
- `IA32_STAR` / `IA32_LSTAR` – kernel/user entry points.
- `IA32_FMASK` – mask flag bits when transitioning.

When adding new MSR consumers (e.g., TSC deadline timer, perf counters), ensure the caller validates feature support via `cpuid` before touching the relevant register.
