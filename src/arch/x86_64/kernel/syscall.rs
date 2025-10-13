#![cfg(target_arch = "x86_64")]

use crate::drivers::console;
use crate::klog;
use core::slice;
use super::msr;

extern "C" {
    fn syscall_entry();
}

#[repr(C)]
pub struct SyscallFrame {
    pub r9: u64,
    pub r8: u64,
    pub r10: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rax: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl SyscallFrame {
    const fn empty() -> Self {
        Self {
            r9: 0,
            r8: 0,
            r10: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rax: 0,
            rip: 0,
            rflags: 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn syscall_trampoline(frame: *mut SyscallFrame) -> u64 {
    let frame = unsafe { &mut *frame };
    dispatch(frame)
}

fn dispatch(frame: &mut SyscallFrame) -> u64 {
    match frame.rax {
        0 => {
            let ptr = frame.rdi as *const u8;
            let len = frame.rsi as usize;
            if ptr.is_null() {
                return u64::MAX;
            }
            let slice = unsafe { slice::from_raw_parts(ptr, len) };
            match console::write_bytes(slice) {
                Ok(count) => count as u64,
                Err(_) => u64::MAX,
            }
        }
        _ => u64::MAX,
    }
}

pub fn write_console(bytes: &[u8]) -> u64 {
    let mut frame = SyscallFrame::empty();
    frame.rax = 0;
    frame.rdi = bytes.as_ptr() as u64;
    frame.rsi = bytes.len() as u64;
    dispatch(&mut frame)
}

pub fn init() {
    unsafe {
        const IA32_EFER: u32 = 0xC000_0080;
        const IA32_STAR: u32 = 0xC000_0081;
        const IA32_LSTAR: u32 = 0xC000_0082;
        const IA32_FMASK: u32 = 0xC000_0084;

        let mut efer = msr::read(IA32_EFER);
        efer |= 1 << 0; // enable syscall/sysret
        msr::write(IA32_EFER, efer);

        let star = ((0x08u64) << 48) | ((0x1Bu64) << 32);
        msr::write(IA32_STAR, star);

        msr::write(IA32_LSTAR, syscall_entry as u64);

        // mask off TF(8) and IF(9)
        msr::write(IA32_FMASK, (1 << 8) | (1 << 9));

        klog!("[syscall] syscall/sysret configured\n");
    }
}
