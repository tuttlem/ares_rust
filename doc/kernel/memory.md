# Memory Management

This document covers physical memory discovery, frame allocation, and the kernel heap.

## Physical memory map (`src/arch/x86_64/kernel/mem/phys.rs`)

- Parses the Multiboot memory map, recording up to 128 usable regions (page-aligned, excluding the first MiB).
- Logs a summary of available regions during boot (`[phys] ...`).
- Provides `allocate_frame()` / `allocate_frames()` to hand out 4 KiB frames via a simple bump allocator that walks the recorded regions.
- `free_frame()` is currently a no-op; the allocator is monotonic, which is sufficient for the kernel’s current use cases.
- `for_each_region` and `summary` expose read-only views of the discovered map for diagnostics.

## Heap (`src/kernel/mem/heap.rs`)

- Implements a linked-list allocator backed by a 1 MiB static array.
- Uses a `SpinLock<LinkedListAllocator>` to provide mutual exclusion between tasks.
- Supports `allocate`/`deallocate` with splitting and coalescing (`merge_with_next/previous`).
- `heap::init()` seeds the allocator and runs a small self-test in `kmain`.

## Alignment helpers

Both layers provide `align_up` / `align_down` utilities to keep frame and allocation addresses aligned to required boundaries.

## Future considerations

- Reclaiming frames: once the kernel supports process teardown with address space isolation, `free_frame` will need a real implementation.
- Larger heaps or per-process allocators can build on top of the frame allocator by requesting contiguous spans (`allocate_frames`).
- MMU paging structures are currently assumed to be configured by the bootloader; future work could extend this module to manage page tables directly (see `doc/kernel/mmu.md`).
