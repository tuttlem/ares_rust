#![no_std]

#[path = "../arch/mod.rs"]
pub mod arch;

mod interrupts;
mod klog;
mod drivers;
mod mem;
mod syscall;
mod sync;
mod timer;
mod cpu;
pub mod process;

use core::alloc::Layout;
use core::ffi::c_void;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::ptr;
use core::str;

use crate::mem::heap::{self, HeapBox};

#[no_mangle]
pub extern "C" fn kmain(multiboot_info: *const c_void, multiboot_magic: u32) -> ! {
    let info_addr = multiboot_info as usize;

    klog::init();
    klog!("[kmain] multiboot magic: 0x{:08X}
", multiboot_magic);
    klog!("[kmain] multiboot info ptr: 0x{:016X}
", info_addr);

    interrupts::init();
    mem::phys::init(info_addr);
    heap::init();
    drivers::init();
    drivers::register_builtin();
    drivers::list_drivers();
    drivers::self_test();
    let init_pid = process::init().expect("process init");
    klog!("[process] init pid={}\n", init_pid);
    syscall::init();
    let banner = b"[ares] Booting Ares kernel\n";
    let _ = syscall::write(syscall::fd::STDOUT, banner);

    let before = heap::remaining_bytes();
    {
        let mut boxed = HeapBox::new([0u64; 8]).expect("heap alloc");
        for (i, slot) in boxed.iter_mut().enumerate() {
            *slot = i as u64;
        }
        let sum: u64 = boxed.iter().copied().sum();
        klog!("[heap] array sum={} first={} last={}
", sum, boxed[0], boxed[7]);
    }
    let after = heap::remaining_bytes();
    klog!("[heap] remaining before={} after={}
", before, after);

    unsafe {
        let layout = Layout::from_size_align(64, 16).unwrap();
        let ptr = heap::allocate(layout);
        if !ptr.is_null() {
            ptr::write_bytes(ptr, 0xA5, 64);
            heap::deallocate(ptr, layout);
            klog!("[heap] manual alloc/free ok
");
        } else {
            klog!("[heap] manual allocation failed
");
        }
    }

    timer::init();

    let vendor_raw = cpu::vendor_string();
    let vendor = str::from_utf8(&vendor_raw).unwrap_or("unknown");
    klog!("[kmain] CPU vendor: {vendor}
");
    klog!("[kmain] CPUID max basic leaf: 0x{:08X}
", cpu::highest_basic_leaf());
    klog!("[kmain] CPUID max extended leaf: 0x{:08X}
", cpu::highest_extended_leaf());

    let features = cpu::features();
    klog!("[kmain] CPUID feature ECX: 0x{:08X}
", features.ecx);
    klog!("[kmain] CPUID feature EDX: 0x{:08X}
", features.edx);

    if features.has_edx(cpu::feature::edx::SSE) && features.has_edx(cpu::feature::edx::SSE2) {
        klog::writeln("[kmain] SSE/SSE2 supported");
    }

    if features.has_ecx(cpu::feature::ecx::AVX) {
        klog::writeln("[kmain] AVX supported");
    }

    interrupts::enable();

    let mut input_buf = [0u8; 64];
    loop {
        let count = syscall::read(syscall::fd::STDIN, &mut input_buf);
        if count > 0 && count <= input_buf.len() as u64 {
            let slice = &input_buf[..count as usize];
            let _ = syscall::write(syscall::fd::STDOUT, slice);
        }
        spin_loop();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    klog::writeln("[kpanic] Kernel panic!");
    klog!("[kpanic] {}
", info);

    loop {
        spin_loop();
    }
}
