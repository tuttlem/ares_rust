#![cfg(target_arch = "x86_64")]

use crate::klog;
use crate::process;
use crate::process::ProcessError;
use core::slice;
use super::msr;

pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const YIELD: u64 = 24; // matches Linux sched_yield
    pub const EXIT: u64 = 60;  // matches Linux exit
}

pub mod fd {
    pub const STDIN: u64 = 0;
    pub const STDOUT: u64 = 1;
    pub const STDERR: u64 = 2;
    pub const SCRATCH: u64 = 3;
}

const ERR_BADF: u64 = u64::MAX - 0;
const ERR_FAULT: u64 = u64::MAX - 1;
const ERR_NOSYS: u64 = u64::MAX - 2;

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
        nr::READ => sys_read(frame.rdi, frame.rsi, frame.rdx),
        nr::WRITE => sys_write(frame.rdi, frame.rsi, frame.rdx),
        nr::YIELD => sys_yield(),
        nr::EXIT => sys_exit(frame.rdi),
        _ => ERR_NOSYS,
    }
}

fn sys_read(fd: u64, buf_ptr: u64, len: u64) -> u64 {
    if len == 0 {
        return 0;
    }

    if buf_ptr == 0 {
        return ERR_FAULT;
    }

    let len = len as usize;
    let buffer = unsafe { slice::from_raw_parts_mut(buf_ptr as *mut u8, len) };

    let current_pid = match process::current_pid() {
        Some(pid) => pid,
        None => {
            klog!("[syscall] read with no current process pid fd={} len={}\n", fd, len);
            return ERR_BADF;
        }
    };

    match process::with_fd_mut(current_pid, fd as usize, |descriptor| descriptor.read(buffer)) {
        Ok(Ok(count)) => count as u64,
        Ok(Err(_)) => ERR_BADF,
        Err(ProcessError::InvalidFileDescriptor) => {
            klog!("[syscall] pid {} missing fd {}\n", current_pid, fd);
            ERR_BADF
        }
        Err(err) => {
            klog!("[syscall] read failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            ERR_BADF
        }
    }
}

fn sys_write(fd: u64, buf_ptr: u64, len: u64) -> u64 {
    if len == 0 {
        return 0;
    }

    if buf_ptr == 0 {
        return ERR_FAULT;
    }

    let len = len as usize;
    let slice = unsafe { slice::from_raw_parts(buf_ptr as *const u8, len) };

    let current_pid = match process::current_pid() {
        Some(pid) => pid,
        None => {
            klog!("[syscall] write with no current process pid fd={} len={}\n", fd, len);
            return ERR_BADF;
        }
    };

    match process::with_fd_mut(current_pid, fd as usize, |descriptor| descriptor.write(slice)) {
        Ok(Ok(count)) => count as u64,
        Ok(Err(_)) => ERR_BADF,
        Err(ProcessError::InvalidFileDescriptor) => {
            klog!("[syscall] pid {} missing fd {}\n", current_pid, fd);
            ERR_BADF
        }
        Err(err) => {
            klog!("[syscall] write failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            ERR_BADF
        }
    }
}

fn sys_yield() -> u64 {
    process::yield_now();
    0
}

fn sys_exit(code: u64) -> u64 {
    let status = (code & 0xFFFF_FFFF) as i32;
    process::exit_current(status);
}

pub fn write(fd: u64, bytes: &[u8]) -> u64 {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::WRITE;
    frame.rdi = fd;
    frame.rsi = bytes.as_ptr() as u64;
    frame.rdx = bytes.len() as u64;
    dispatch(&mut frame)
}

pub fn read(fd: u64, buf: &mut [u8]) -> u64 {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::READ;
    frame.rdi = fd;
    frame.rsi = buf.as_mut_ptr() as u64;
    frame.rdx = buf.len() as u64;
    dispatch(&mut frame)
}

pub fn yield_now() {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::YIELD;
    let _ = dispatch(&mut frame);
}

pub fn exit(status: i32) -> ! {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::EXIT;
    frame.rdi = status as u64;
    let _ = dispatch(&mut frame);
    loop {
        core::hint::spin_loop();
    }
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
