# Ares Kernel Overview

This document captures the high-level structure of the Ares 64-bit kernel. Detailed subsystem notes live alongside this file inside the `doc/` tree.

- **Architecture split** – Portable logic lives in `src/kernel`, while CPU/board specific code is under `src/arch/x86_64`.
- **Bootstrap path** – Multiboot hands control to the assembly entry in `arch/x86_64/boot/main.asm`, which sets up paging-friendly state before jumping into Rust (`kmain`). A full timeline is described in [`boot.md`](boot.md).
- **Drivers** – Character drivers are layered: architecture shims live in `arch/x86_64/drivers`, exposed through registry-backed facades in `kernel/drivers`. See the individual notes under `drivers/`.
- **Processes & scheduling** – Kernel processes own dedicated stacks, contexts, file descriptors, and tracked heap regions. A cooperative scheduler is augmented with timer-driven preemption. The lifecycle, table layout, and context switching details are summarised in [`kernel/process.md`](kernel/process.md) and [`kernel/context_switching.md`](kernel/context_switching.md).
- **Interrupts & syscalls** – The Interrupt Descriptor Table (IDT), PIC remapping, and ISR stub glue are covered in [`kernel/interrupts.md`](kernel/interrupts.md). System-call setup (STAR/LSTAR/EFER MSRs and the dispatcher) is captured in [`kernel/syscall.md`](kernel/syscall.md).
- **Timer & preemption** – The PIT is programmed via `pit.rs` and drives the tick counter plus preemption requests. Behavioural notes are in [`kernel/pit.md`](kernel/pit.md) and [`kernel/timer.md`](kernel/timer.md).
- **Memory** – Physical memory discovery, frame allocation, and the heap allocator are outlined in [`kernel/memory.md`](kernel/memory.md). Low-level helpers for MMU registers and MSRs are covered separately.
- **Development** – Building, running, and available tooling are summarised in [`development.md`](development.md).

These documents are intended to be living notes—update them when subsystems grow new capabilities or change behaviour.
