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
}

pub use self::ports::{inb, outb};
