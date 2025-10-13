#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::syscall::*;

#[cfg(not(target_arch = "x86_64"))]
pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
}

#[cfg(not(target_arch = "x86_64"))]
pub mod fd {
    pub const STDIN: u64 = 0;
    pub const STDOUT: u64 = 1;
    pub const STDERR: u64 = 2;
}

#[cfg(not(target_arch = "x86_64"))]
pub fn init() {}

#[cfg(not(target_arch = "x86_64"))]
pub fn read(_fd: u64, _buf: &mut [u8]) -> u64 {
    0
}

#[cfg(not(target_arch = "x86_64"))]
pub fn write(_fd: u64, bytes: &[u8]) -> u64 {
    bytes.len() as u64
}
