#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("Architecture module not implemented for this target");
