#![no_std]

mod interrupts;
mod klog;
mod mem;
mod sync;
mod timer;
mod cpu;

use core::ffi::c_void;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::str;

#[no_mangle]
pub extern "C" fn kmain(multiboot_info: *const c_void, multiboot_magic: u32) -> ! {
    let info_addr = multiboot_info as usize;

    klog::init();
    klog::writeln("[kmain] Booting Ares kernel");
    klog!("[kmain] multiboot magic: 0x{:08X}\n", multiboot_magic);
    klog!("[kmain] multiboot info ptr: 0x{:016X}\n", info_addr);

    interrupts::init();
    mem::phys::init(info_addr);
    mem::heap::init();
    timer::init();

    let vendor_raw = cpu::vendor_string();
    let vendor = str::from_utf8(&vendor_raw).unwrap_or("unknown");
    klog!("[kmain] CPU vendor: {vendor}\n");
    klog!("[kmain] CPUID max basic leaf: 0x{:08X}\n", cpu::highest_basic_leaf());
    klog!("[kmain] CPUID max extended leaf: 0x{:08X}\n", cpu::highest_extended_leaf());

    let features = cpu::features();
    klog!("[kmain] CPUID feature ECX: 0x{:08X}\n", features.ecx);
    klog!("[kmain] CPUID feature EDX: 0x{:08X}\n", features.edx);

    if features.has_edx(cpu::feature::edx::SSE) && features.has_edx(cpu::feature::edx::SSE2) {
        klog::writeln("[kmain] SSE/SSE2 supported");
    }

    if features.has_ecx(cpu::feature::ecx::AVX) {
        klog::writeln("[kmain] AVX supported");
    }

    interrupts::enable();

    loop {
        spin_loop();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    klog::writeln("[kpanic] Kernel panic!");
    klog!("[kpanic] {}\n", info);

    loop {
        spin_loop();
    }
}
