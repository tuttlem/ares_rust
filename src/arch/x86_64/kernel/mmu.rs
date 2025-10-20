pub(crate) unsafe fn read_cr2() -> u64 {
    let value: u64;
    core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, preserves_flags));
    value
}

pub(crate) unsafe fn read_cr3() -> u64 {
    let value: u64;
    core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, preserves_flags));
    value
}

pub(crate) unsafe fn write_cr3(value: u64) {
    core::arch::asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags));
}

pub(crate) const KERNEL_VMA_BASE: u64 = 0xFFFF_8000_0000_0000;

pub(crate) fn phys_to_virt(phys: u64) -> u64 {
    phys + KERNEL_VMA_BASE
}

pub(crate) fn virt_to_phys(virt: u64) -> u64 {
    virt - KERNEL_VMA_BASE
}
