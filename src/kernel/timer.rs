#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::timer::*;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("timer implementation not available for this architecture");
