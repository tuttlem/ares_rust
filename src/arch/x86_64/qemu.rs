use core::hint::spin_loop;
use klog;

pub fn exit(code: u8) -> ! {
    klog!("[exit] kernel exiting qemu: {}\n", code);

    unsafe { super::io::outb(0xF4, code); }
    loop {
        spin_loop();
    }
}

pub fn exit_success() -> ! {
    exit(0)
}

pub fn exit_failure() -> ! {
    exit(1)
}
