#![cfg(kernel_test)]

use crate::drivers::{BlockDevice, Driver, DriverError, DriverKind};
use crate::sync::spinlock::SpinLock;

pub struct TestBlockDevice<const N: usize> {
    name: &'static str,
    block_size: usize,
    storage: SpinLock<[u8; N]>,
}

impl<const N: usize> TestBlockDevice<N> {
    pub const fn new(name: &'static str, block_size: usize) -> Self {
        Self {
            name,
            block_size,
            storage: SpinLock::new([0; N]),
        }
    }

    pub fn reset(&self) {
        let mut guard = self.storage.lock();
        guard.fill(0);
    }

    pub fn load_image(&self, data: &[u8]) -> Result<(), &'static str> {
        let mut guard = self.storage.lock();
        if data.len() > guard.len() {
            return Err("image too large");
        }
        guard[..data.len()].copy_from_slice(data);
        Ok(())
    }

}

impl<const N: usize> Driver for TestBlockDevice<N> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Block
    }

    fn init(&self) -> Result<(), DriverError> {
        Ok(())
    }
}

impl<const N: usize> BlockDevice for TestBlockDevice<N> {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), DriverError> {
        if buf.len() % self.block_size() != 0 {
            return Err(DriverError::Unsupported);
        }
        let offset = (lba as usize)
            .checked_mul(self.block_size())
            .ok_or(DriverError::IoError)?;
        let end = offset + buf.len();
        let guard = self.storage.lock();
        if end > guard.len() {
            return Err(DriverError::IoError);
        }
        buf.copy_from_slice(&guard[offset..end]);
        Ok(())
    }

    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), DriverError> {
        if buf.len() % self.block_size() != 0 {
            return Err(DriverError::Unsupported);
        }
        let offset = (lba as usize)
            .checked_mul(self.block_size())
            .ok_or(DriverError::IoError)?;
        let end = offset + buf.len();
        let mut guard = self.storage.lock();
        if end > guard.len() {
            return Err(DriverError::IoError);
        }
        guard[offset..end].copy_from_slice(buf);
        Ok(())
    }

    fn flush(&self) -> Result<(), DriverError> {
        Ok(())
    }
}
