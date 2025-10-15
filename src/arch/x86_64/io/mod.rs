#![allow(dead_code)]

pub mod ports {
    #[inline(always)]
    pub unsafe fn outb(port: u16, value: u8) {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }

    #[inline(always)]
    pub unsafe fn inb(port: u16) -> u8 {
        let value: u8;
        core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
        value
    }

    #[inline(always)]
    pub unsafe fn outw(port: u16, value: u16) {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
    }

    #[inline(always)]
    pub unsafe fn inw(port: u16) -> u16 {
        let value: u16;
        core::arch::asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack, preserves_flags));
        value
    }

    #[inline(always)]
    pub unsafe fn insw(port: u16, buffer: *mut u16, count: usize) {
        if count == 0 {
            return;
        }
        core::arch::asm!(
            "rep insw",
            in("dx") port,
            inout("rdi") buffer => _,
            inout("rcx") count => _,
            options(nostack)
        );
    }

    #[inline(always)]
    pub unsafe fn outsw(port: u16, buffer: *const u16, count: usize) {
        if count == 0 {
            return;
        }
        core::arch::asm!(
            "rep outsw",
            in("dx") port,
            inout("rsi") buffer => _,
            inout("rcx") count => _,
            options(nostack)
        );
    }
}

pub use self::ports::{inb, inw, insw, outb, outsw, outw};
