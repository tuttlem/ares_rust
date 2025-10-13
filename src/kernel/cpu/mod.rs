#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::cpu::*;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("cpuid module is not implemented for this architecture");
