# Built-in Character Devices & Registry

Portable layer: `src/kernel/drivers/`.

## Registry (`mod.rs`)

- Maintains a dynamic list of registered `CharDevice` instances.
- Provides helper functions used by the syscall layer to resolve file descriptors to devices (`lookup`, `register`, `keyboard()`/`console()` accessors).
- Wraps devices in `Arc<dyn CharDevice>` so multiple processes can reference the same endpoint.

## Built-in devices (`builtin.rs`)

The kernel ships with three default character devices:

| Device | File descriptor | Behaviour |
|--------|-----------------|-----------|
| Console | STDOUT/STDERR (1/2) | Writes to the VGA text buffer and mirrors to serial. |
| Keyboard | STDIN (0) | Provides buffered input from the PS/2 driver. |
| `/dev/null` | Not exposed by default FD table | Discards writes, returns EOF on reads. |
| `/dev/zero` | Not exposed by default FD table | Returns zeroed bytes, accepts and ignores writes. |

The initialization path (`drivers::init()`) registers these devices so they are available to the kernel scheduler and syscalls.

## Extending the registry

To add a new device:

1. Implement `CharDevice` in either the portable layer or wrap an architecture-specific helper.
2. Call `drivers::register_builtin` (or similar) during boot to populate the registry.
3. Update documentation here and wire the device into the default FD table if appropriate.
