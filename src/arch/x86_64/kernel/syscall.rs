#![cfg(target_arch = "x86_64")]

use crate::drivers::DriverError;
use crate::klog;
use crate::process;
use crate::process::{FileIoError, ProcessError, SeekFrom};
use crate::vfs::VfsError;
use core::{slice, str};
use super::msr;

pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const SEEK: u64 = 8;
    pub const YIELD: u64 = 24; // matches Linux sched_yield
    pub const EXIT: u64 = 60;  // matches Linux exit
}

pub mod fd {
    pub const STDIN: u64 = 0;
    pub const STDOUT: u64 = 1;
    pub const STDERR: u64 = 2;
    pub const SCRATCH: u64 = 3;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SeekWhence {
    Set,
    Cur,
    End,
}

impl SeekWhence {
    fn as_raw(self) -> u64 {
        match self {
            SeekWhence::Set => 0,
            SeekWhence::Cur => 1,
            SeekWhence::End => 2,
        }
    }
}

const ERR_BADF: u64 = u64::MAX - 0;
const ERR_FAULT: u64 = u64::MAX - 1;
const ERR_NOSYS: u64 = u64::MAX - 2;
const ERR_INVAL: u64 = u64::MAX - 3;
const ERR_NOENT: u64 = u64::MAX - 4;
const ERR_NOMEM: u64 = u64::MAX - 5;
const ERR_IO: u64 = u64::MAX - 6;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SysError {
    BadFileDescriptor,
    Fault,
    NoSys,
    InvalidArgument,
    NoEntry,
    NoMemory,
    Io,
}

pub type SysResult<T> = Result<T, SysError>;

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
        nr::OPEN => sys_open(frame.rdi, frame.rsi, frame.rdx),
        nr::CLOSE => sys_close(frame.rdi),
        nr::SEEK => sys_seek(frame.rdi, frame.rsi, frame.rdx),
        nr::YIELD => sys_yield(),
        nr::EXIT => sys_exit(frame.rdi),
        _ => ERR_NOSYS,
    }
}

fn decode_ret(value: u64) -> SysResult<u64> {
    match value {
        ERR_BADF => Err(SysError::BadFileDescriptor),
        ERR_FAULT => Err(SysError::Fault),
        ERR_NOSYS => Err(SysError::NoSys),
        ERR_INVAL => Err(SysError::InvalidArgument),
        ERR_NOENT => Err(SysError::NoEntry),
        ERR_NOMEM => Err(SysError::NoMemory),
        ERR_IO => Err(SysError::Io),
        other => Ok(other),
    }
}

fn encode_error(err: SysError) -> u64 {
    match err {
        SysError::BadFileDescriptor => ERR_BADF,
        SysError::Fault => ERR_FAULT,
        SysError::NoSys => ERR_NOSYS,
        SysError::InvalidArgument => ERR_INVAL,
        SysError::NoEntry => ERR_NOENT,
        SysError::NoMemory => ERR_NOMEM,
        SysError::Io => ERR_IO,
    }
}

fn decode_seek(offset: u64, whence: u64) -> SysResult<SeekFrom> {
    match whence {
        0 => {
            if (offset as i64) < 0 {
                Err(SysError::InvalidArgument)
            } else {
                Ok(SeekFrom::Start(offset))
            }
        }
        1 => Ok(SeekFrom::Current(offset as i64)),
        2 => Ok(SeekFrom::End(offset as i64)),
        _ => Err(SysError::InvalidArgument),
    }
}

fn map_file_io_error(err: FileIoError) -> SysError {
    match err {
        FileIoError::Driver(DriverError::Unsupported) => SysError::InvalidArgument,
        FileIoError::Driver(DriverError::IoError) => SysError::Io,
        FileIoError::Driver(DriverError::RegistryFull) => SysError::NoMemory,
        FileIoError::Driver(DriverError::InitFailed) => SysError::Io,
        FileIoError::Vfs(VfsError::Unsupported) => SysError::InvalidArgument,
        FileIoError::Vfs(VfsError::InvalidOffset) => SysError::InvalidArgument,
        FileIoError::Vfs(VfsError::Io) => SysError::Io,
    }
}

fn sys_open(path_ptr: u64, path_len: u64, _flags: u64) -> u64 {
    if path_ptr == 0 || path_len == 0 {
        return ERR_INVAL;
    }

    let slice = unsafe { slice::from_raw_parts(path_ptr as *const u8, path_len as usize) };
    let trimmed = match slice.iter().position(|&b| b == 0) {
        Some(pos) => &slice[..pos],
        None => slice,
    };
    let path_str = match str::from_utf8(trimmed) {
        Ok(s) => s,
        Err(_) => return ERR_INVAL,
    };

    let current_pid = match process::current_pid() {
        Some(pid) => pid,
        None => return ERR_BADF,
    };

    match process::open_path(current_pid, path_str) {
        Ok(fd) => fd as u64,
        Err(ProcessError::NoFreeFileDescriptors) => encode_error(SysError::NoMemory),
        Err(ProcessError::PathNotFound) => encode_error(SysError::NoEntry),
        Err(ProcessError::InvalidFileDescriptor) => encode_error(SysError::BadFileDescriptor),
        Err(err) => {
            klog!("[syscall] open failed pid {} path {:?} err {:?}\n", current_pid, path_str, err);
            encode_error(SysError::BadFileDescriptor)
        }
    }
}

fn sys_close(fd: u64) -> u64 {
    let current_pid = match process::current_pid() {
        Some(pid) => pid,
        None => return ERR_BADF,
    };

    match process::close_fd(current_pid, fd as usize) {
        Ok(()) => 0,
        Err(ProcessError::InvalidFileDescriptor) => encode_error(SysError::BadFileDescriptor),
        Err(err) => {
            klog!("[syscall] close failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            encode_error(SysError::BadFileDescriptor)
        }
    }
}

fn sys_seek(fd: u64, offset: u64, whence: u64) -> u64 {
    let current_pid = match process::current_pid() {
        Some(pid) => pid,
        None => return ERR_BADF,
    };

    let seek_from = match decode_seek(offset, whence) {
        Ok(seek) => seek,
        Err(err) => return encode_error(err),
    };

    match process::with_fd_mut(current_pid, fd as usize, |descriptor| descriptor.seek(seek_from)) {
        Ok(Ok(new_offset)) => new_offset,
        Ok(Err(err)) => {
            let sys_err = map_file_io_error(err);
            klog!(
                "[syscall] seek: device error {:?} (fd={} offset={} whence={})\n",
                sys_err,
                fd,
                offset,
                whence
            );
            encode_error(sys_err)
        }
        Err(ProcessError::InvalidFileDescriptor) => encode_error(SysError::BadFileDescriptor),
        Err(err) => {
            klog!("[syscall] seek failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            encode_error(SysError::BadFileDescriptor)
        }
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
        Ok(Err(err)) => encode_error(map_file_io_error(err)),
        Err(ProcessError::InvalidFileDescriptor) => encode_error(SysError::BadFileDescriptor),
        Err(err) => {
            klog!("[syscall] read failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            encode_error(SysError::BadFileDescriptor)
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
        Ok(Err(err)) => encode_error(map_file_io_error(err)),
        Err(ProcessError::InvalidFileDescriptor) => encode_error(SysError::BadFileDescriptor),
        Err(err) => {
            klog!("[syscall] write failed pid {} fd {} err {:?}\n", current_pid, fd, err);
            encode_error(SysError::BadFileDescriptor)
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

pub fn write(fd: u64, bytes: &[u8]) -> SysResult<usize> {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::WRITE;
    frame.rdi = fd;
    frame.rsi = bytes.as_ptr() as u64;
    frame.rdx = bytes.len() as u64;
    decode_ret(dispatch(&mut frame)).map(|value| value as usize)
}

pub fn read(fd: u64, buf: &mut [u8]) -> SysResult<usize> {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::READ;
    frame.rdi = fd;
    frame.rsi = buf.as_mut_ptr() as u64;
    frame.rdx = buf.len() as u64;
    decode_ret(dispatch(&mut frame)).map(|value| value as usize)
}

pub fn open(path: &str) -> SysResult<usize> {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::OPEN;
    frame.rdi = path.as_ptr() as u64;
    frame.rsi = path.len() as u64;
    frame.rdx = 0;
    decode_ret(dispatch(&mut frame)).map(|value| value as usize)
}

pub fn close(fd: u64) -> SysResult<()> {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::CLOSE;
    frame.rdi = fd;
    decode_ret(dispatch(&mut frame)).map(|_| ())
}

pub fn seek(fd: u64, offset: i64, whence: SeekWhence) -> SysResult<u64> {
    let mut frame = SyscallFrame::empty();
    frame.rax = nr::SEEK;
    frame.rdi = fd;
    frame.rsi = offset as u64;
    frame.rdx = whence.as_raw();
    decode_ret(dispatch(&mut frame))
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
