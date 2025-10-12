#[inline]
pub unsafe fn write(msr: u32, value: u64) {
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") (value & 0xFFFF_FFFF) as u32,
        in("edx") (value >> 32) as u32,
        options(nostack, preserves_flags),
    );
}

#[inline]
pub unsafe fn read(msr: u32) -> u64 {
    let high: u32;
    let low: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("edx") high,
        out("eax") low,
        options(nostack, preserves_flags),
    );
    ((high as u64) << 32) | low as u64
}
