#![cfg(target_arch = "x86_64")]

use crate::klog;
use crate::process;
use crate::process::FileDescriptor;
use core::slice;
use super::msr;

pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
}

pub mod fd {
    pub const STDIN: u64 = 0;
    pub const STDOUT: u64 = 1;
    pub const STDERR: u64 = 2;
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

    let descriptor = match process::descriptor(current_pid, fd as usize) {
        Some(desc) => desc,
        None => {
            klog!("[syscall] pid {} missing fd {}\n", current_pid, fd);
            return ERR_BADF;
        }
    };

    read_via_descriptor(descriptor, buffer)
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

    let descriptor = match process::descriptor(current_pid, fd as usize) {
        Some(desc) => desc,
        None => {
            klog!("[syscall] pid {} missing fd {}\n", current_pid, fd);
            return ERR_BADF;
        }
    };

    write_via_descriptor(descriptor, slice)
}

fn write_via_descriptor(descriptor: FileDescriptor, slice: &[u8]) -> u64 {
    match descriptor.write(slice) {
        Ok(count) => count as u64,
        Err(_) => ERR_BADF,
    }
}

fn read_via_descriptor(descriptor: FileDescriptor, buf: &mut [u8]) -> u64 {
    match descriptor.read(buf) {
        Ok(count) => count as u64,
        Err(_) => ERR_BADF,
    }
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
