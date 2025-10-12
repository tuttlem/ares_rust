#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
mod arch;

#[cfg(target_arch = "x86_64")]
pub fn init() {
    arch::init();
}

#[cfg(not(target_arch = "x86_64"))]
pub fn init() {}

#[cfg(target_arch = "x86_64")]
pub fn write_console(bytes: &[u8]) -> u64 {
    arch::write_console(bytes)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn write_console(bytes: &[u8]) -> u64 {
    let _ = bytes;
    0
}
