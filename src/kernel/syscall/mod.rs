#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::syscall::*;
#[cfg(target_arch = "x86_64")]
#[allow(unused_imports)]
pub use crate::arch::x86_64::kernel::syscall::{SysError, SysResult};

#[cfg(not(target_arch = "x86_64"))]
pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const SEEK: u64 = 8;
    pub const YIELD: u64 = 24;
    pub const EXIT: u64 = 60;
}

#[cfg(not(target_arch = "x86_64"))]
pub mod fd {
    pub const STDIN: u64 = 0;
    pub const STDOUT: u64 = 1;
    pub const STDERR: u64 = 2;
    pub const SCRATCH: u64 = 3;
}

#[cfg(not(target_arch = "x86_64"))]
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

#[cfg(not(target_arch = "x86_64"))]
pub type SysResult<T> = Result<T, SysError>;

#[cfg(not(target_arch = "x86_64"))]
pub fn init() {}

#[cfg(not(target_arch = "x86_64"))]
pub fn read(_fd: u64, _buf: &mut [u8]) -> SysResult<usize> {
    Ok(0)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn write(_fd: u64, bytes: &[u8]) -> SysResult<usize> {
    Ok(bytes.len())
}

#[cfg(not(target_arch = "x86_64"))]
pub fn open(_path: &str) -> SysResult<usize> {
    Ok(0)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn close(_fd: u64) -> SysResult<()> {
    Ok(())
}

#[cfg(not(target_arch = "x86_64"))]
pub fn seek(_fd: u64, _offset: u64) -> SysResult<u64> {
    Ok(0)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn yield_now() {}

#[cfg(not(target_arch = "x86_64"))]
pub fn exit(_code: i32) -> ! {
    loop {}
}
