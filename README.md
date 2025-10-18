# Ares

64-bit Development Operating System

## Status Overview

Ares has grown into a small but usable 64-bit kernel written primarily in Rust with a handful of x86_64 assembly stubs. It currently provides:

- A clear separation between platform-agnostic kernel code (`src/kernel`) and architecture-specific support (`src/arch/x86_64`).
- Multiboot-based bootstrap, IDT and PIC initialisation, and a cooperative scheduler for kernel processes.
- A dynamic driver subsystem with architecture-specific character drivers (console, serial, keyboard) registered at runtime.
- A syscall layer exposing `read`/`write` that honours per-process file descriptor tables (STDIN/STDOUT/STDERR default to keyboard/console devices).
- Buffered PS/2 keyboard input so user keystrokes are delivered via the syscall path and echoed to the console.
- VGA console output with a software cursor overlay, plus serial logging through the same driver abstraction.
- Per-process kernel stacks, region-aware heap allocations, and tooling to introspect process state for debugging.

These pieces are already enough to boot into a cooperative multi-tasking kernel where multiple tasks interleave via explicit yields.

## Architecture Overview

```
src/
 ├── arch/
 │   └── x86_64/
 │       ├── boot/                # Multiboot entry and low-level assembly stubs
 │       ├── drivers/             # VGA console, serial, PS/2 keyboard backends
 │       ├── io/                  # Shared port I/O helpers (inb/outb)
 │       └── kernel/              # Interrupts, PIT, syscall trampolines, context switch
 └── kernel/
    ├── drivers/                 # Driver registry and architecture-neutral facades
    ├── fs/                      # Filesystem modules (FAT, future drivers) plugged into the VFS
    ├── process/                 # Process table, scheduler, descriptors, memory regions
     ├── mem/                     # Kernel heap allocator and physical memory parsing
     ├── syscall/                 # High-level syscall facade
     ├── timer/, cpu/, sync/      # Supporting subsystems
     └── kmain.rs                 # Kernel entry point
```

### Boot Flow

1. Multiboot hands control to `kmain`, which sets up interrupts, physical memory tracking, and the kernel heap.
2. Drivers are registered through a dynamic registry that sources architecture implementations (console, serial, keyboard).
3. The syscall layer configures `syscall/sysret` MSRs and the init banner is written via the new syscall path.
4. `process::init()` prepares the process table and allocates per-process stacks with tracked memory regions.
5. Sample kernel processes (the keyboard echo shell and a ticker heartbeat) are spawned, after which the cooperative scheduler takes over.

### Process Runtime & Scheduling

- Each process owns a dedicated kernel stack (16 KiB), a saved `Context`, and a descriptor table for standard streams.
- `process::spawn_kernel_process` seeds descriptors, registers stack regions, and schedules tasks through a round-robin dispatcher.
- An assembly stub (`context_switch.asm`) saves callee-saved registers plus `rsp/rip/rflags` before jumping into the next task.
- Diagnostics such as `process::dump_process`, `dump_current_process`, and `dump_all_processes` log registers, stack addresses, descriptors, and memory regions over serial.
- Helper APIs (`allocate_for_process`, `free_for_process`) wrap heap allocations so regions are tracked per PID.

### Syscalls & File Descriptors

- Syscall numbers follow the Linux convention for `read`/`write`, with descriptors 0/1/2 mapped to keyboard, console, and console respectively.
- The dispatcher validates the current PID, resolves the descriptor from the process table, and delegates to the underlying driver, returning sentinel errors for bad descriptors or missing processes.
- Kernel tasks exercise the same ABI (e.g., the echo shell reads from STDIN and writes to STDOUT exclusively via syscalls).

### Device Drivers & I/O

- Character drivers live under `src/kernel/drivers` and adapt to architecture backends; the registry expands dynamically using the kernel heap.
- The console driver manages VGA text memory, scrolling, and a non-blinking software cursor overlay.
- The keyboard driver translates set-1 scancodes, maintains a ring buffer, and feeds interrupts into the syscall layer.
- A shared `io` module centralises `inb`/`outb`, removing duplicated inline assembly across PIT, PIC, console, serial, and keyboard code.

### Virtual File System & FAT support

- The VFS traits now live under `src/kernel/vfs`, with `/dev/null`, `/dev/zero`, `/scratch`, and `/fat/...` routed through the same descriptor table.
- `src/kernel/fs/fat.rs` provides a read-only FAT12/16 implementation that mounts a volume at boot (default LBA `4096`).  It exposes 8.3 files in the root directory through the VFS so `open("/fat/NAME.EXT")` Just Works.
- Boot-time smoke tests in `ticker_task_a` write to `/dev/null`, read `/dev/zero`, hit `/scratch`, and (if present) log the contents of `/fat/HELLO.TXT`.

### Memory Management & Diagnostics

- Physical memory is parsed from Multiboot tables and logged during boot.
- A linked-list heap allocator backs kernel allocations; process stacks and additional regions are carved out from this heap.
- Per-process region tracking lays the groundwork for future per-PID heaps or address-space isolation.
- Serial logging (`klog!`) provides a unified way to emit diagnostics from anywhere in the kernel.

## Development

Build environment is provided as a docker container. Create the docker image with the following:

```
docker build buildenv -t ares_build
```

### Preparing the FAT test volume

The kernel expects a FAT16 filesystem beginning at sector 4096 (2 MiB) inside the raw disk image.  Format it and copy a test file with:

```
mkfs.fat --offset=4096 -F 16 -n ARESFAT dist/x86_64/hda.img
mcopy -i dist/x86_64/hda.img@@2097152 TEST.TXT ::HELLO.TXT
```

(`2097152 = 4096 × 512` bytes.)  On the next boot you should see a log message similar to:

```
[fat] mounted volume at LBA 4096
[vfs:smoke] /fat/HELLO.TXT read 12 bytes: Hello World!
```

You can now build the project with the following:

```
./domake build-x86_64
```

### Testing

- **Host-side unit tests:**

  The `ares-core` crate mirrors the kernel's pure Rust subsystems and exposes them behind a `std` feature so they can be exercised on the host. Run the suite (FAT parsing, VFS scratch file, etc.) with:

  ```
  cargo test -p ares-core
  ```

  The FAT tests build an in-memory disk image, while the VFS tests use a mocked block device; no special tooling is required beyond a standard Rust toolchain.

- **Kernel integration tests:**

  The kernel ships with a minimal in-kernel harness guarded by `--cfg kernel_test`. Build and execute it under QEMU (using the ISA debug-exit device for pass/fail reporting) with:

  ```
  make qemu-test
  ```

  The harness initialises the heap and process table, runs a handful of smoke tests (heap allocations, process spawning), and then exits via `outb(0xF4, code)`. A zero exit code indicates success; non-zero values correspond to the number of failing tests. You can target a subset of checks with `FILTER`, e.g. `make qemu-test FILTER=process` or `make qemu-test FILTER=memory.heap_allocation`.

## Running

Using qemu, you can you use the following:

```
qemu-system-x86_64 -cdrom ./dist/x86_64/kernel.iso
```
