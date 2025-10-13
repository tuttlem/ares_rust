#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::interrupts::*;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("interrupt handling not implemented for this architecture");
