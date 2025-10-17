use crate::drivers::{BlockDevice, Driver, DriverError, DriverKind};

pub struct MemBlockDevice {
    name: &'static str,
    block: usize,
    storage: std::sync::Mutex<Vec<u8>>,
}

impl MemBlockDevice {
    pub fn new(name: &'static str, data: Vec<u8>, block_size: usize) -> Self {
        assert_eq!(data.len() % block_size, 0, "backing must align to block size");
        Self {
            name,
            block: block_size,
            storage: std::sync::Mutex::new(data),
        }
    }

    fn with_storage<R>(&self, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        let mut guard = self.storage.lock().expect("mem block device poisoned");
        f(&mut guard)
    }
}

impl Driver for MemBlockDevice {
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

impl BlockDevice for MemBlockDevice {
    fn block_size(&self) -> usize {
        self.block
    }

    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), DriverError> {
        let block_size = self.block_size();
        if buf.len() % block_size != 0 {
            return Err(DriverError::Unsupported);
        }
        let offset = (lba as usize)
            .checked_mul(block_size)
            .ok_or(DriverError::IoError)?;

        self.with_storage(|storage| {
            let end = offset + buf.len();
            if end > storage.len() {
                return Err(DriverError::IoError);
            }
            buf.copy_from_slice(&storage[offset..end]);
            Ok(())
        })
    }

    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), DriverError> {
        let block_size = self.block_size();
        if buf.len() % block_size != 0 {
            return Err(DriverError::Unsupported);
        }
        let offset = (lba as usize)
            .checked_mul(block_size)
            .ok_or(DriverError::IoError)?;

        self.with_storage(|storage| {
            let end = offset + buf.len();
            if end > storage.len() {
                return Err(DriverError::IoError);
            }
            storage[offset..end].copy_from_slice(buf);
            Ok(())
        })
    }
}
