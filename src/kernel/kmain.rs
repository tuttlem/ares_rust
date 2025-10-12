#![no_std]

mod klog;

use core::ffi::c_void;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

#[no_mangle]
pub extern "C" fn kmain(multiboot_info: *const c_void, multiboot_magic: u32) -> ! {
    let info_addr = multiboot_info as usize;

    klog::init();
    klog::writeln("[kmain] Booting Ares kernel");
    klog!("[kmain] multiboot magic: 0x{:08X}\n", multiboot_magic);
    klog!("[kmain] multiboot info ptr: 0x{:016X}\n", info_addr);

    write_banner(b"Welcome to Ares (Rust kmain)\0");

    loop {
        spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    klog::writeln("[kpanic] Kernel panic!");
    write_banner(b"Kernel panic!\0");

    loop {
        spin_loop();
    }
}

fn write_banner(message: &[u8]) {
    const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
    const ATTR: u16 = 0x0F00; // bright white on black
    const SIZE: usize = 80 * 25;
    const BLANK: u16 = ATTR | 0x20;

    unsafe {
        let mut idx = 0usize;
        while idx < SIZE {
            write_volatile(VGA_BUFFER.add(idx), BLANK);
            idx += 1;
        }

        let mut msg_idx = 0usize;
        while msg_idx < message.len() && msg_idx < SIZE {
            let ch = message[msg_idx];
            if ch == 0 {
                break;
            }

            let value = ATTR | ch as u16;
            write_volatile(VGA_BUFFER.add(msg_idx), value);
            msg_idx += 1;
        }
    }
}
