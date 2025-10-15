# Interrupt Handling

Sources:
- `src/arch/x86_64/kernel/interrupts.rs`
- `src/arch/x86_64/kernel/interrupts.asm`

## Initialisation

`interrupts::init()` performs the following steps:

1. Builds the IDT array in Rust, wiring architecture stubs for vectors 0–47.
2. Registers high-level handlers for page faults (14) and general protection faults (13).
3. Remaps the PIC (master @ 0x20, slave @ 0x28) so hardware IRQs do not clash with CPU exceptions.
4. Loads the IDTR via the `idt_stub_load` assembly helper.

After `init()`, `interrupts::enable()` issues `sti` to globally enable interrupts.

## Assembly stubs (`interrupts.asm`)

- Provide `isr_*` and `irq_*` entry points that save general-purpose registers, push an error code (0 for implicit vectors), and jump into `isr_handler`/`irq_handler` in Rust.
- Use `push_all`/`pop_all` macros to save/restore callee-saved registers alongside the scratch registers.
- Freeze interrupts with `cli` on entry and restore with `sti` before `iretq`.

## Dispatch in Rust

`isr_handler` / `irq_handler` simply call `dispatch(frame)`, selecting a handler from the `HANDLERS` table that can be replaced at runtime (`register_handler`).

### Built-in handlers

- **Page fault** – Reads `cr2` to log the faulting linear address and decodes the error bits (present/write/user/reserved/instruction).
- **General protection fault** – Logs the faulting RIP, CS, RFLAGS, and dumps the current process (if any) for diagnostics.
- **PIT / keyboard IRQs** – Registered by the timer and keyboard subsystems respectively.

The PIC EOI is sent automatically in `irq_handler` after running the handler.

## Preemption hook

Timer interrupts call `process::request_preempt`, which:

1. Marks `NEED_RESCHED`.
2. Checks the interrupted RIP is canonical and resides in kernel space.
3. Records the preempt return address on the running process and patches the interrupted frame to jump into `preempt_trampoline`.

Later, when the trampoline executes, `preempt_do_switch` performs the actual context switch (see `doc/kernel/context_switching.md`).

## Adding new handlers

- Register with `interrupts::register_handler(vector, handler_fn)` after `interrupts::init()`.
- Enable hardware IRQ lines via `interrupts::enable_vector(vector)` when needed.
- Keep handlers short; defer longer work to a dedicated process or bottom-half.
