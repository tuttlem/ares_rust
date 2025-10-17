#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate std;

pub mod drivers;
pub mod klog;
pub mod mem;
pub mod sync;
pub mod vfs;

pub mod fs {
    pub mod fat;
}
