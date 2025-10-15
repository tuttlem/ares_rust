# MMU Helpers

File: `src/arch/x86_64/kernel/mmu.rs`.

The kernel currently relies on the bootloader to enable paging with a suitable higher-half mapping. Only a minimal utility exists:

- `unsafe fn read_cr2() -> u64` – returns the faulting linear address on page faults.

Future paging extensions (e.g., building and switching page tables, manipulating CR3/CR4) should live alongside this helper. The simplified model keeps the rest of the kernel agnostic to the underlying paging structures for now.
