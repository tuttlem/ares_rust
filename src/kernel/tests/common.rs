#![cfg(kernel_test)]

use core::sync::atomic::{AtomicBool, Ordering};

use crate::drivers::{BlockDevice, Driver, DriverError, DriverKind};
use crate::fs::fat;
use crate::sync::spinlock::SpinLock;
use crate::vfs::ata::AtaScratchFile;

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

const BLOCK_SIZE: usize = 512;
const SCRATCH_CAPACITY: usize = BLOCK_SIZE * 4;
const FAT_CAPACITY: usize = BLOCK_SIZE * 12;

pub static SCRATCH_DEVICE: TestBlockDevice<SCRATCH_CAPACITY> =
    TestBlockDevice::new("test-scratch", BLOCK_SIZE);
pub static FAT_DEVICE: TestBlockDevice<FAT_CAPACITY> =
    TestBlockDevice::new("test-fat", BLOCK_SIZE);

static SCRATCH_READY: AtomicBool = AtomicBool::new(false);
static FAT_READY: AtomicBool = AtomicBool::new(false);

pub fn init_scratch() {
    if SCRATCH_READY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        SCRATCH_DEVICE.reset();
        unsafe {
            AtaScratchFile::init(&SCRATCH_DEVICE, 0, "ata0-scratch");
        }
    }
}

pub fn mount_hello() -> Result<(), &'static str> {
    if FAT_READY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        let image = hello_image();
        FAT_DEVICE.reset();
        FAT_DEVICE
            .load_image(&image)
            .map_err(|_| "fat image too large")?;
        fat::mount(&FAT_DEVICE, 0).map_err(|_| "fat mount failed")?;
    }
    Ok(())
}

fn hello_image() -> [u8; BLOCK_SIZE * 10] {
    let mut image = [0u8; BLOCK_SIZE * 10];

    {
        let bpb = &mut image[0..BLOCK_SIZE];
        bpb[11..13].copy_from_slice(&(BLOCK_SIZE as u16).to_le_bytes());
        bpb[13] = 1;
        bpb[14..16].copy_from_slice(&(1u16).to_le_bytes());
        bpb[16] = 1;
        bpb[17..19].copy_from_slice(&(16u16).to_le_bytes());
        bpb[21] = 0xF8;
        bpb[22..24].copy_from_slice(&(1u16).to_le_bytes());
        bpb[24..26].copy_from_slice(&(1u16).to_le_bytes());
        bpb[26..28].copy_from_slice(&(1u16).to_le_bytes());
        bpb[510] = 0x55;
        bpb[511] = 0xAA;
    }

    {
        let fat = &mut image[BLOCK_SIZE..BLOCK_SIZE * 2];
        fat[0] = 0xF8;
        fat[1] = 0xFF;
        fat[2] = 0xFF;
        fat[3] = 0xFF;
        let cluster2 = 2 * 2;
        fat[cluster2..cluster2 + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    }

    {
        let root = &mut image[BLOCK_SIZE * 2..BLOCK_SIZE * 3];
        root[0..11].copy_from_slice(b"HELLO   TXT");
        root[11] = 0x20;
        root[26..28].copy_from_slice(&(2u16).to_le_bytes());
        root[28..32].copy_from_slice(&(5u32).to_le_bytes());
    }

    {
        let data = &mut image[BLOCK_SIZE * 3..BLOCK_SIZE * 4];
        data[..5].copy_from_slice(b"Hello");
    }

    image
}
