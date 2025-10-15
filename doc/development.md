# Building & Development Workflow

## Tooling assumptions

- The project builds inside the Docker image defined under `buildenv/`. The helper script `./domake` wraps `docker run`, mounts the repository, and runs `make` as an unprivileged user.
- Host requirements: Docker, GNU `make`, and QEMU for testing the resulting ISO.

## Build targets

| Command | Description |
|---------|-------------|
| `./domake build-x86_64` | Compile the kernel and assemble the bootable ISO into `dist/x86_64/`. |
| `./domake clean` | Remove build artifacts (`build/`, `dist/`). |

The `Makefile` drives `cargo xbuild` and the NASM assembly passes for the boot stubs/context switcher.

## Running under QEMU

For interactive serial output on the host terminal:

```bash
qemu-system-x86_64 \
  -cdrom dist/x86_64/kernel.iso \
  -serial stdio \
  -no-reboot
```

For long-running tests it is often preferable to redirect the UART to a file (to avoid stdout throttling):

```bash
qemu-system-x86_64 \
  -cdrom dist/x86_64/kernel.iso \
  -serial mon:stdio \
  -serial file:kernel.log \
  -display none \
  -no-reboot
```

## Logging & diagnostics

- `klog!` prints reach both the VGA console and the serial port via the console driver.
- The process subsystem exposes `dump_all_processes()` to record task state and scheduler statistics.
- Timer interrupts (100 Hz) are enabled by default and request preemption; the scheduler emits `[request_preempt]` / `[preempt]` log lines when context switches occur.

## Directory structure recap

- `src/kernel/` – architecture-neutral logic (processes, scheduler, heap, syscall façade, etc.).
- `src/arch/x86_64/` – assembly and Rust glue for the x86_64 platform (boot, drivers, interrupts, low-level I/O helpers).
- `dist/x86_64/` – build artifacts (ISO + raw kernel binary).
- `doc/` – this documentation set.

When adding new subsystems, remember to:

1. Update the appropriate document under `doc/`.
2. Export the functionality through the existing architecture/portable layers (e.g., new drivers should plumb through `kernel/drivers`).
3. Add logging guardrails to keep the serial output manageable.
