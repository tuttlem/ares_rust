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
    let data = fs::read_binary(path).map_err(LoaderError::File)?;
    let image = elf::parse(&data).map_err(LoaderError::Elf)?;
    Ok((image, data))
}
