# Serial (COM1) Driver

Source: `src/arch/x86_64/drivers/serial.rs` with a portable wrapper in `src/kernel/drivers/console.rs` (for mirroring) and `klog`.

## Responsibilities

- Initialise the 16550-compatible UART on COM1 (0x3F8).
- Provide polled transmit routines for byte output.
- Expose a simple initialisation API used during early boot (before interrupts are enabled).

## Key functions

- `init()` – disables interrupts, configures baud (38400), line control (8N1), FIFO state, and modem control bits.
- `write_byte(byte)` – translates `\n` into CRLF, then calls `transmit`.
- `transmit(byte)` – busy-waits on `is_transmit_empty()` before writing to the DATA register.
- `is_transmit_empty()` – reads the line-status register (0x3FD) and tests bit 5.

## Considerations

- The implementation is intentionally simple: it polls in a tight loop with `spin_loop()` hints. Excessive logging can therefore stall progress if the host cannot drain the UART fast enough.
- Future work could introduce interrupt-driven TX or throttling to avoid starving other tasks.
- The driver is used both for kernel logging (`klog!`) and the console mirror; keep their combined throughput in mind when enabling verbose logs.
