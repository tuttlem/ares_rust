use alloc::vec::Vec;

pub use super::elf::{self, ElfImage};
pub use super::fs::FileError;
use super::fs;

#[derive(Debug)]
pub enum LoaderError {
    File(FileError),
    Elf(elf::ElfError),
}

pub fn load_elf(path: &str) -> Result<(ElfImage, Vec<u8>), LoaderError> {
    crate::klog!("[loader] load_elf path='{}'\n", path);
    let data = fs::read_binary(path).map_err(|err| {
        crate::klog!("[loader] read_binary failed: {:?}\n", err);
        LoaderError::File(err)
    })?;
    crate::klog!("[loader] read_binary ok size={} bytes\n", data.len());
    let image = elf::parse(&data).map_err(LoaderError::Elf)?;
    crate::klog!("[loader] elf parse ok entry=0x{:016X} segments={}\n", image.entry, image.segments.len());
    Ok((image, data))
}
