use crate::arch::x86_64::io::outb;

const PIT_CLOCK_OSC: u32 = 1_193_182;

pub(crate) fn init_frequency(hz: u32) {
    assert!(hz > 0, "PIT frequency must be greater than zero");

    let mut divisor = PIT_CLOCK_OSC / hz;
    if divisor == 0 {
        divisor = 1;
    }
    if divisor > u16::MAX as u32 {
        divisor = u16::MAX as u32;
    }

    let low = (divisor & 0xFF) as u8;
    let high = ((divisor >> 8) & 0xFF) as u8;

    unsafe {
        outb(0x43, 0x36);
        outb(0x40, low);
        outb(0x40, high);
    }
}
