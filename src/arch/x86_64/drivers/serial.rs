use core::hint::spin_loop;

const COM1_PORT: u16 = 0x3F8;

const DATA: u16 = COM1_PORT;
const INTERRUPT_ENABLE: u16 = COM1_PORT + 1;
const FIFO_CONTROL: u16 = COM1_PORT + 2;
const LINE_CONTROL: u16 = COM1_PORT + 3;
const MODEM_CONTROL: u16 = COM1_PORT + 4;
const LINE_STATUS: u16 = COM1_PORT + 5;

pub(crate) fn init() {
    unsafe {
        outb(INTERRUPT_ENABLE, 0x00); // disable interrupts
        outb(LINE_CONTROL, 0x80);     // enable DLAB

        // Set baud to 115200 / 3 = 38400
        outb(DATA, 0x03);             // divisor low byte
        outb(INTERRUPT_ENABLE, 0x00); // divisor high byte

        outb(LINE_CONTROL, 0x03);     // 8 bits, no parity, one stop bit
        outb(FIFO_CONTROL, 0xC7);     // enable FIFO, clear them, 14-byte threshold
        outb(MODEM_CONTROL, 0x0B);    // IRQs enabled, RTS/DSR set
    }
}

pub(crate) fn write_byte(byte: u8) {
    if byte == b'\n' {
        transmit(b'\r');
    }
    transmit(byte);
}

fn transmit(byte: u8) {
    while !is_transmit_empty() {
        spin_loop();
    }

    unsafe {
        outb(DATA, byte);
    }
}

fn is_transmit_empty() -> bool {
    unsafe { inb(LINE_STATUS) & 0x20 != 0 }
}

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    value
}
