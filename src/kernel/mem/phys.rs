#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::mem::phys::*;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("physical memory manager not implemented for this architecture");
