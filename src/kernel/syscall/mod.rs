#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::syscall::*;

#[cfg(not(target_arch = "x86_64"))]
pub fn init() {}

#[cfg(not(target_arch = "x86_64"))]
pub fn write_console(bytes: &[u8]) -> u64 {
    let _ = bytes;
    0
}
