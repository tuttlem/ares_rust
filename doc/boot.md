# Boot Process

This note walks through the execution path from the firmware hand-off into the Rust kernel.

## Multiboot entry (`src/arch/x86_64/boot/main.asm`)

1. **Multiboot header** – The loader jumps directly into the 64-bit entry point exported by `main.asm`. The assembly sets up segment registers, a temporary stack, and clears `.bss`.
2. **CPU state** – Paging is already active (courtesy of the loader), but the stub ensures we run with the expected GDT selectors loaded.
3. **Rust hand-off** – After creating a clean stack, the stub tail-calls `kmain(multiboot_info, multiboot_magic)`.

## Kernel initialisation (`src/kernel/kmain.rs`)

The top-level initialisation sequence looks like:

1. **Console logging** – `klog::init()` wires the logging macros to the console/serial drivers.
2. **Interrupts** – `interrupts::init()` remaps the PIC, allocates the IDT, and installs architecture handlers (see `doc/kernel/interrupts.md`).
3. **Physical memory discovery** – `mem::phys::init()` parses the Multiboot memory map, records usable regions, and initialises the bump-based frame allocator.
4. **Heap** – `heap::init()` seeds a 1 MiB heap managed by the linked-list allocator (`src/kernel/mem/heap.rs`). Diagnostic allocations validate the allocator.
5. **Drivers** – `drivers::init()` registers architecture shims (console, keyboard, serial).
6. **Process table** – `process::init()` creates the idle task and readies the process table.
7. **Syscalls** – `syscall::init()` programs the IA32_* MSRs to point to the fast syscall trampolines and enables the `syscall/sysret` instruction pair.
8. **Timer** – `timer::init()` configures the PIT to 100 Hz and registers the timer interrupt handler.
9. **Sample processes** – `kmain` spawns:
   - `init`: a simple echo shell.
   - `ticker_a/b/c`: heartbeat loggers exercising the scheduler.
   - `dump_all`: periodic process table dumps.
   - `parent`: repeatedly spawns and waits on a short-lived `worker` task.

10. **Interrupts on & scheduler** – After enabling interrupts (`interrupts::enable()`), `process::start_scheduler()` never returns. From this point onward, task switches are handled by the scheduler combined with timer-driven preemption.

With this pipeline complete the kernel is fully operational: consoles are live, the heap is available, interrupts are configured, and cooperative/preemptive multitasking is active.
