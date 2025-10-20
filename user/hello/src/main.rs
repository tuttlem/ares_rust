#![no_std]
#![no_main]

use core::arch::asm;

const MSG: &[u8] = b"Hello from ring3!\n";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        syscall_write(1, MSG.as_ptr(), MSG.len());
        syscall_exit(0);
    }
}

unsafe fn syscall_write(fd: u64, buf: *const u8, len: usize) {
    asm!(
        "syscall",
        inlateout("rax") 1u64 => _,
        in("rdi") fd,
        in("rsi") buf,
        in("rdx") len as u64,
        lateout("rcx") _,
        lateout("r11") _,
    );
}

unsafe fn syscall_exit(code: u64) -> ! {
    asm!(
        "syscall",
        in("rax") 60u64,
        in("rdi") code,
        options(noreturn)
    );
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { syscall_exit(1) }
}
