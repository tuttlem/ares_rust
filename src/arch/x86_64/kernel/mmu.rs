pub(crate) unsafe fn read_cr2() -> u64 {
    let value: u64;
    core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, preserves_flags));
    value
}
