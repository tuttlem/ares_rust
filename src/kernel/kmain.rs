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
mod vfs;
pub mod process;

use core::alloc::Layout;
use core::ffi::c_void;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::ptr;
use core::str;

use crate::mem::heap::{self, HeapBox};
use crate::vfs::ata::AtaScratchFile;
use crate::vfs::VfsFile;

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

    let vendor_raw = cpu::vendor_string();
    let vendor = str::from_utf8(&vendor_raw).unwrap_or("unknown");
    klog!("[kmain] CPU vendor: {vendor}\n");
    klog!("[kmain] CPUID max basic leaf: 0x{:08X}\n", cpu::highest_basic_leaf());
    klog!("[kmain] CPUID max extended leaf: 0x{:08X}\n", cpu::highest_extended_leaf());

    let features = cpu::features();
    klog!("[kmain] CPUID feature ECX: 0x{:08X}\n", features.ecx);
    klog!("[kmain] CPUID feature EDX: 0x{:08X}\n", features.edx);

    if features.has_edx(cpu::feature::edx::SSE) && features.has_edx(cpu::feature::edx::SSE2) {
        unsafe { cpu::enable_sse(); }
        klog::writeln("[kmain] SSE/SSE2 enabled");
    } else {
        klog::writeln("[kmain] SSE/SSE2 unavailable");
    }

    if features.has_ecx(cpu::feature::ecx::AVX) {
        klog::writeln("[kmain] AVX supported");
    }

    drivers::register_builtin();
    drivers::list_drivers();
    if let Some(ata_dev) = drivers::block_device_by_name("ata0-master") {
        unsafe {
            let file = AtaScratchFile::init(ata_dev, 2048, "ata0-scratch");
            klog!("[vfs] scratch file '{}' mounted at LBA {}\n", file.name(), 2048);
        }
    } else {
        klog!("[vfs] ata0-master unavailable; scratch file not initialised\n");
    }
    process::init().expect("process init");
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

    process::spawn_kernel_process("init", init_shell_task).expect("spawn init");
    process::spawn_kernel_process("ticker_a", ticker_task_a).expect("spawn ticker_a");
    process::spawn_kernel_process("ticker_b", ticker_task_b).expect("spawn ticker_b");
    process::spawn_kernel_process("ticker_c", ticker_task_c).expect("spawn ticker_c");
    process::spawn_kernel_process("dump_all", dump_all).expect("dump_all");
    process::spawn_kernel_process("parent", parent_task).expect("spawn parent");
    process::spawn_kernel_process("vfs_smoke", vfs_smoke_task).expect("spawn vfs_smoke");

    interrupts::enable();

    process::start_scheduler();
}

extern "C" fn init_shell_task() -> ! {
    let mut input_buf = [0u8; 64];
    loop {
        let count = syscall::read(syscall::fd::STDIN, &mut input_buf);
        if count == 0 {
            process::yield_now();
            continue;
        }
        if count <= input_buf.len() as u64 {
            let slice = &input_buf[..count as usize];
            let _ = syscall::write(syscall::fd::STDOUT, slice);
        }
        process::yield_now();
    }
}

extern "C" fn ticker_task_a() -> ! {
    ticker_loop("A", b"[ticker-A] heartbeat\n")
}

extern "C" fn ticker_task_b() -> ! {
    ticker_loop("B", b"[ticker-B] heartbeat\n")
}

extern "C" fn ticker_task_c() -> ! {
    ticker_loop("C", b"[ticker-C] heartbeat\n")
}

extern "C" fn vfs_smoke_task() -> ! {
    let ok = crate::vfs::tests::scratch_smoke_test();
    let code = if ok { 0 } else { 1 };
    syscall::exit(code);
}

fn ticker_loop(_name: &'static str, stdout_msg: &'static [u8]) -> ! {
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        /*
        klog!(
            "[ticker-{name}] heartbeat count={} tick={}\n",
            counter,
            timer::ticks()
        );
        */
        if counter % 32 == 0 {
            let _ = syscall::write(syscall::fd::STDOUT, stdout_msg);
        }
        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
        syscall::yield_now();
    }
}

extern "C" fn dump_all() -> ! {
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 5_500 == 0 {
            process::dump_all_processes();
        }
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
        process::yield_now();
    }
}

extern "C" fn parent_task() -> ! {
    let mut iteration: u64 = 0;
    loop {
        iteration = iteration.wrapping_add(1);
        let child_pid = match process::spawn_kernel_process("worker", worker_task) {
            Ok(pid) => pid,
            Err(err) => {
                klog!("[parent] failed to spawn worker: {:?}\n", err);
                process::yield_now();
                continue;
            }
        };

        klog!("[parent] spawned worker pid={} iteration={}\n", child_pid, iteration);

        match process::wait_for_child(Some(child_pid)) {
            Ok((pid, code)) => {
                klog!("[parent] worker pid={} exit_code={}\n", pid, code);
            }
            Err(err) => {
                klog!("[parent] wait failed: {:?}\n", err);
            }
        }

        process::yield_now();
    }
}

extern "C" fn worker_task() -> ! {
    let mut iterations: u32 = 0;
    let msg = b"[worker] tick\n";
    loop {
        if iterations >= 3 {
            syscall::exit(0);
        }
        iterations += 1;
        klog!(
            "[worker] tick iteration={} pid={:?} tick={}\n",
            iterations,
            process::current_pid(),
            timer::ticks()
        );
        if iterations % 2 == 1 {
            let _ = syscall::write(syscall::fd::STDOUT, msg);
        }
        for _ in 0..15_000 {
            core::hint::spin_loop();
        }
        syscall::yield_now();
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
