use crate::drivers::DriverError;

/// Result alias for VFS operations.
pub type VfsResult<T> = core::result::Result<T, VfsError>;

/// Minimal error type for virtual file system interactions.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum VfsError {
    Io,
    Unsupported,
    InvalidOffset,
}

impl From<DriverError> for VfsError {
    fn from(_: DriverError) -> Self {
        VfsError::Io
    }
}

/// Behaviour common to readable/writable file-like objects in the kernel.
pub trait VfsFile: Sync {
    fn name(&self) -> &'static str;

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;

    fn flush(&self) -> VfsResult<()>;

    fn size(&self) -> VfsResult<u64>;
}

pub mod ata;
pub mod tests;
