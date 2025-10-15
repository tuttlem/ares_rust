# Port I/O Helpers

The module `src/arch/x86_64/io/mod.rs` provides a minimal wrapper around the x86 `in`/`out` instructions:

- `unsafe fn outb(port: u16, value: u8)` – writes a byte to an I/O port (`out dx, al`).
- `unsafe fn inb(port: u16) -> u8` – reads a byte from an I/O port (`in al, dx`).

Both helpers are `#[inline(always)]` and marked `options(nomem, nostack, preserves_flags)` to keep the compiler aware of their side effects.

All drivers that touch legacy hardware (VGA cursor, PS/2 keyboard, COM1 UART, PIT) depend on these helpers. Keeping them in one module ensures:

- call sites have a common `unsafe` boundary;
- the operations can be stubbed or instrumented in future if we port to environments without raw port access.

When adding new port-level interactions prefer to wrap them in a driver-specific helper instead of scattering `asm!` blocks across the tree.
