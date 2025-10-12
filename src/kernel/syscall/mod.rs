#![allow(dead_code)]

use crate::klog;

#[cfg(target_arch = "x86_64")]
#[path = "../../arch/x86_64/kernel/msr.rs"]
mod msr;

#[cfg(target_arch = "x86_64")]
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

#[no_mangle]
extern "C" fn syscall_trampoline(frame: *mut SyscallFrame) -> u64 {
    let frame = unsafe { &mut *frame };
    dispatch(frame)
}

fn dispatch(frame: &mut SyscallFrame) -> u64 {
    match frame.rax {
        0 => {
            klog!("[syscall] hello from syscall 0\n");
            0
        }
        _ => u64::MAX,
    }
}

pub fn init() {
    #[cfg(target_arch = "x86_64")]
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
