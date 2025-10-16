use crate::drivers::BlockDevice;

use super::{VfsError, VfsFile, VfsResult};

const SCRATCH_BYTES: usize = 512;

static mut SCRATCH_FILE: Option<AtaScratchFile> = None;

pub struct AtaScratchFile {
    device: &'static dyn BlockDevice,
    lba: u64,
    name: &'static str,
}

impl AtaScratchFile {
    pub fn new(device: &'static dyn BlockDevice, lba: u64, name: &'static str) -> Self {
        Self { device, lba, name }
    }

    pub unsafe fn init(device: &'static dyn BlockDevice, lba: u64, name: &'static str) -> &'static AtaScratchFile {
        SCRATCH_FILE = Some(Self::new(device, lba, name));
        SCRATCH_FILE.as_ref().unwrap()
    }

    pub fn get() -> Option<&'static AtaScratchFile> {
        unsafe { SCRATCH_FILE.as_ref() }
    }

    pub fn bytes_per_sector(&self) -> usize {
        self.sector_size()
    }

    fn sector_size(&self) -> usize {
        self.device.block_size()
    }

    fn ensure_scratch_capacity(&self) -> VfsResult<()> {
        if self.sector_size() > SCRATCH_BYTES {
            return Err(VfsError::Unsupported);
        }
        Ok(())
    }
}

impl VfsFile for AtaScratchFile {
    fn name(&self) -> &'static str {
        self.name
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.ensure_scratch_capacity()?;
        if buf.is_empty() {
            return Ok(0);
        }

        let sector_size = self.sector_size();
        if offset >= sector_size as u64 {
            return Err(VfsError::InvalidOffset);
        }

        let start = offset as usize;
        if start + buf.len() > sector_size {
            return Err(VfsError::Unsupported);
        }

        let mut sector = [0u8; SCRATCH_BYTES];
        self.device
            .read_blocks(self.lba, &mut sector[..sector_size])
            .map_err(VfsError::from)?;

        buf.copy_from_slice(&sector[start..start + buf.len()]);
        Ok(buf.len())
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.ensure_scratch_capacity()?;
        if buf.is_empty() {
            return Ok(0);
        }

        let sector_size = self.sector_size();
        if offset >= sector_size as u64 {
            return Err(VfsError::InvalidOffset);
        }

        let start = offset as usize;
        if start + buf.len() > sector_size {
            return Err(VfsError::Unsupported);
        }

        let mut sector = [0u8; SCRATCH_BYTES];
        self.device
            .read_blocks(self.lba, &mut sector[..sector_size])
            .map_err(VfsError::from)?;

        sector[start..start + buf.len()].copy_from_slice(buf);
        self.device
            .write_blocks(self.lba, &sector[..sector_size])
            .map_err(VfsError::from)?;
        self.device.flush().map_err(VfsError::from)?;
        Ok(buf.len())
    }

    fn flush(&self) -> VfsResult<()> {
        self.device.flush().map_err(VfsError::from)
    }

    fn size(&self) -> VfsResult<u64> {
        self.ensure_scratch_capacity()?;
        Ok(self.sector_size() as u64)
    }
}
