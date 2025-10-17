#![allow(dead_code)]

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DriverKind {
    Block,
    Char,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DriverError {
    RegistryFull,
    InitFailed,
    Unsupported,
    IoError,
}

pub trait Driver: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> DriverKind;
    fn init(&self) -> Result<(), DriverError>;
    fn shutdown(&self) {}
}

pub trait BlockDevice: Driver {
    fn block_size(&self) -> usize;
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), DriverError>;
    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), DriverError>;

    fn flush(&self) -> Result<(), DriverError> {
        Ok(())
    }
}

pub trait CharDevice: Driver {
    fn read(&self, buf: &mut [u8]) -> Result<usize, DriverError>;
    fn write(&self, buf: &[u8]) -> Result<usize, DriverError>;
}

#[cfg(any(test, feature = "std"))]
pub mod mock;
