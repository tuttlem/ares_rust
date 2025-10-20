#![no_std]

#[path = "../arch/mod.rs"]
pub mod arch;

mod interrupts;
mod klog;
mod drivers;
mod fs;
mod mem;
mod syscall;
mod sync;
mod timer;
mod cpu;
mod vfs;
pub mod process;
#[cfg(kernel_test)]
mod tests;

#[cfg(not(kernel_test))]
use core::alloc::Layout;
use core::ffi::c_void;
use core::hint::spin_loop;
use core::panic::PanicInfo;
#[cfg(not(kernel_test))]
use core::ptr;
use core::str;

use crate::mem::heap;
#[cfg(not(kernel_test))]
use crate::mem::heap::HeapBox;
const FAT_START_LBA: u64 = 4096;
#[cfg(not(kernel_test))]
use crate::vfs::ata::AtaScratchFile;
#[cfg(not(kernel_test))]
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

    #[cfg(kernel_test)]
    tests::run(info_addr);

    #[cfg(not(kernel_test))]
    {
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
            match fs::fat::mount(ata_dev, FAT_START_LBA) {
                Ok(()) => klog!("[fat] mounted volume at LBA {}\n", FAT_START_LBA),
                Err(err) => klog!("[fat] mount failed: {:?}\n", err),
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
        klog!("[heap] array sum={} first={} last={}\n", sum, boxed[0], boxed[7]);
    }
    let after = heap::remaining_bytes();
    klog!("[heap] remaining before={} after={}\n", before, after);

    unsafe {
        let layout = Layout::from_size_align(64, 16).unwrap();
        let ptr = heap::allocate(layout);
        if !ptr.is_null() {
            ptr::write_bytes(ptr, 0xA5, 64);
            heap::deallocate(ptr, layout);
            klog!("[heap] manual alloc/free ok\n");
        } else {
            klog!("[heap] manual allocation failed\n");
        }
    }

        timer::init();

    process::spawn_kernel_process("init", init_shell_task).expect("spawn init");

        interrupts::enable();

        process::start_scheduler();
    }
}

extern "C" fn init_shell_task() -> ! {
    let mut input_buf = [0u8; 64];
    loop {
        let count = match syscall::read(syscall::fd::STDIN, &mut input_buf) {
            Ok(count) => count,
            Err(err) => {
                klog!("[shell] read error: {:?}\n", err);
                process::yield_now();
                continue;
            }
        };

        if count == 0 {
            process::yield_now();
            continue;
        }
        if count <= input_buf.len() {
            let slice = &input_buf[..count];
            if let Err(err) = syscall::write(syscall::fd::STDOUT, slice) {
                klog!("[shell] write error: {:?}\n", err);
            }
        }
        process::yield_now();
    }
}

fn vfs_smoke_checks() {
    // --- /dev/null ---
    match syscall::open("/dev/null") {
        Ok(fd) => {
            let fd = fd as u64;
            match syscall::write(fd, b"discard") {
                Ok(written) => klog!("[vfs:smoke] /dev/null accepted {} bytes\n", written),
                Err(err) => klog!("[vfs:smoke] /dev/null write failed: {:?}\n", err),
            }
            if let Err(err) = syscall::close(fd) {
                klog!("[vfs:smoke] /dev/null close failed: {:?}\n", err);
            }
        }
        Err(err) => klog!("[vfs:smoke] open /dev/null failed: {:?}\n", err),
    }

    // --- /dev/zero ---
    match syscall::open("/dev/zero") {
        Ok(fd) => {
            let fd = fd as u64;
            let mut buf = [0xAAu8; 16];
            match syscall::read(fd, &mut buf) {
                Ok(read) => {
                    let mut all_zero = true;
                    for byte in &buf[..read] {
                        if *byte != 0 {
                            all_zero = false;
                            break;
                        }
                    }
                    klog!(
                        "[vfs:smoke] /dev/zero read {} bytes zeros={} data={:02X?}\n",
                        read,
                        all_zero,
                        &buf[..read]
                    );
                }
                Err(err) => klog!("[vfs:smoke] /dev/zero read failed: {:?}\n", err),
            }
            if let Err(err) = syscall::close(fd) {
                klog!("[vfs:smoke] /dev/zero close failed: {:?}\n", err);
            }
        }
        Err(err) => klog!("[vfs:smoke] open /dev/zero failed: {:?}\n", err),
    }

    // --- /scratch seek test ---
    match syscall::open("/scratch") {
        Ok(fd) => {
            let fd = fd as u64;
            let data = b"seektest";
            if let Err(err) = syscall::write(fd, data) {
                klog!("[vfs:smoke] /scratch write failed: {:?}\n", err);
            } else if let Err(err) = syscall::seek(fd, 2, syscall::SeekWhence::Set) {
                klog!("[vfs:smoke] /scratch seek failed: {:?}\n", err);
            } else {
                let mut buf = [0u8; 6];
                match syscall::read(fd, &mut buf) {
                    Ok(read) => klog!(
                        "[vfs:smoke] /scratch read {} bytes -> {:?}\n",
                        read,
                        &buf[..read]
                    ),
                    Err(err) => klog!("[vfs:smoke] /scratch read failed: {:?}\n", err),
                }
            }
            if let Err(err) = syscall::close(fd) {
                klog!("[vfs:smoke] /scratch close failed: {:?}\n", err);
            }
        }
        Err(err) => klog!("[vfs:smoke] open /scratch failed: {:?}\n", err),
    }

    // --- /fat/HELLO.TXT (if present) ---
    match syscall::open("/fat/HELLO.TXT") {
        Ok(fd) => {
            let fd = fd as u64;
            let mut buf = [0u8; 64];
            match syscall::read(fd, &mut buf) {
                Ok(read) => {
                    if let Err(err) = syscall::seek(fd, 0, syscall::SeekWhence::Set) {
                        klog!("[vfs:smoke] /fat seek failed: {:?}\n", err);
                    }
                    let text = core::str::from_utf8(&buf[..read]).unwrap_or("<non-utf8>");
                    klog!("[vfs:smoke] /fat/HELLO.TXT read {} bytes: {}\n", read, text);
                }
                Err(err) => klog!("[vfs:smoke] /fat read failed: {:?}\n", err),
            }
            if let Err(err) = syscall::close(fd) {
                klog!("[vfs:smoke] /fat close failed: {:?}\n", err);
            }
        }
        Err(err) => klog!("[vfs:smoke] open /fat/HELLO.TXT failed: {:?}\n", err),
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
