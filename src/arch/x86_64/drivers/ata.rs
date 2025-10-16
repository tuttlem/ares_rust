use core::hint::spin_loop;
use core::sync::atomic::{compiler_fence, Ordering};

use crate::drivers::{BlockDevice, Driver, DriverError, DriverKind};
use crate::klog;

use super::super::io::{inb, insw, outb, outsw};

const PRIMARY_IO_BASE: u16 = 0x1F0;
const PRIMARY_CTRL_BASE: u16 = 0x3F6;

const REG_DATA: u16 = 0x00;
const REG_ERROR: u16 = 0x01;
const REG_FEATURES: u16 = REG_ERROR;
const REG_SECCOUNT0: u16 = 0x02;
const REG_LBA0: u16 = 0x03;
const REG_LBA1: u16 = 0x04;
const REG_LBA2: u16 = 0x05;
const REG_HDDEVSEL: u16 = 0x06;
const REG_COMMAND: u16 = 0x07;
const REG_STATUS: u16 = REG_COMMAND;

const REG_ALTSTATUS: u16 = 0x00;
const REG_DEVICE_CONTROL: u16 = 0x00;

const STATUS_ERR: u8    = 1 << 0;
const STATUS_DRQ: u8    = 1 << 3;
const STATUS_SRV: u8    = 1 << 4;
const STATUS_DF: u8     = 1 << 5;
const STATUS_RDY: u8    = 1 << 6;
const STATUS_BSY: u8    = 1 << 7;

const CMD_IDENTIFY: u8      = 0xEC;
const CMD_READ_SECTORS: u8  = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_CACHE_FLUSH: u8   = 0xE7;

const SECTOR_BYTES: usize = 512;

pub struct AtaPrimaryMaster;

static ATA_PRIMARY: AtaPrimaryMaster = AtaPrimaryMaster;

impl AtaPrimaryMaster {
    const fn io_base(&self) -> u16 {
        PRIMARY_IO_BASE
    }

    const fn ctrl_base(&self) -> u16 {
        PRIMARY_CTRL_BASE
    }

    fn wait_400ns(&self) {
        // Reading the alternate status port four times delays ~400ns.
        for _ in 0..4 {
            unsafe {
                let _ = inb(self.ctrl_base() + REG_ALTSTATUS);
            }
        }
    }

    fn wait_until(&self, mask: u8, value: u8, timeout: usize) -> Result<(), DriverError> {
        for _ in 0..timeout {
            let status = unsafe { inb(self.io_base() + REG_STATUS) };
            if status & STATUS_BSY == 0 && status & mask == value {
                if status & STATUS_ERR != 0 || status & STATUS_DF != 0 {
                    return Err(DriverError::IoError);
                }
                return Ok(());
            }
            spin_loop();
        }
        Err(DriverError::IoError)
    }

    fn select_drive(&self, lba: u64) {
        let head = ((lba >> 24) & 0x0F) as u8;
        let selector = 0xE0 | head; // 0xE0 selects primary master
        unsafe {
            outb(self.io_base() + REG_HDDEVSEL, selector);
        }
    }

    fn issue_identify(&self) -> Result<(), DriverError> {
        self.select_drive(0);
        self.wait_400ns();

        unsafe {
            outb(self.io_base() + REG_SECCOUNT0, 0);
            outb(self.io_base() + REG_LBA0, 0);
            outb(self.io_base() + REG_LBA1, 0);
            outb(self.io_base() + REG_LBA2, 0);
            outb(self.io_base() + REG_COMMAND, CMD_IDENTIFY);
        }

        let mut status = unsafe { inb(self.io_base() + REG_STATUS) };
        if status == 0 {
            return Err(DriverError::Unsupported);
        }

        while status & STATUS_BSY != 0 {
            status = unsafe { inb(self.io_base() + REG_STATUS) };
        }

        if status & STATUS_ERR != 0 {
            return Err(DriverError::IoError);
        }

        self.wait_until(STATUS_DRQ, STATUS_DRQ, 100_000)?;

        // Drain the IDENTIFY data (256 words) into a scratch buffer.
        let mut scratch = [0u16; 256];
        unsafe {
            insw(
                self.io_base() + REG_DATA,
                scratch.as_mut_ptr(),
                scratch.len(),
            );
        }
        Ok(())
    }

    fn pio_read_sector(&self, lba: u64, buffer: &mut [u8; SECTOR_BYTES]) -> Result<(), DriverError> {
        self.select_drive(lba);
        self.wait_400ns();

        unsafe {
            outb(self.ctrl_base() + REG_DEVICE_CONTROL, 0);
            outb(self.io_base() + REG_SECCOUNT0, 1);
            outb(self.io_base() + REG_LBA0, (lba & 0xFF) as u8);
            outb(self.io_base() + REG_LBA1, ((lba >> 8) & 0xFF) as u8);
            outb(self.io_base() + REG_LBA2, ((lba >> 16) & 0xFF) as u8);
            outb(self.io_base() + REG_COMMAND, CMD_READ_SECTORS);
        }

        self.wait_until(STATUS_DRQ, STATUS_DRQ, 100_000)?;

        unsafe {
            let ptr = buffer.as_mut_ptr() as *mut u16;
            insw(self.io_base() + REG_DATA, ptr, SECTOR_BYTES / 2);
        }
        compiler_fence(Ordering::SeqCst);
        Ok(())
    }

    fn pio_write_sector(&self, lba: u64, buffer: &[u8; SECTOR_BYTES]) -> Result<(), DriverError> {
        // Program drive & taskfile
        self.select_drive(lba);
        self.wait_400ns();

        unsafe {
            // Enable IRQs on device, clear SRST
            outb(self.ctrl_base() + REG_DEVICE_CONTROL, 0);

            outb(self.io_base() + REG_SECCOUNT0, 1);
            outb(self.io_base() + REG_LBA0,  (lba & 0xFF) as u8);
            outb(self.io_base() + REG_LBA1, ((lba >> 8)  & 0xFF) as u8);
            outb(self.io_base() + REG_LBA2, ((lba >> 16) & 0xFF) as u8);
            outb(self.io_base() + REG_COMMAND, CMD_WRITE_SECTORS);
        }

        // Device should become ready to accept data
        // Wait: BSY=0 and DRQ=1; bail if ERR/DF
        self.wait_until(STATUS_DRQ, STATUS_DRQ, 100_000)?;

        // Push 512 bytes (256 words) to the data port
        unsafe {
            let ptr = buffer.as_ptr() as *const u16;
            outsw(self.io_base() + REG_DATA, ptr, SECTOR_BYTES / 2);
        }
        compiler_fence(Ordering::SeqCst);

        // Finalize: wait for BSY=0 and DRQ=0 (transfer complete)
        self.wait_until(STATUS_DRQ, 0, 100_000)?;
        // Check for error bits one last time
        let st = unsafe { inb(self.io_base() + REG_STATUS) };
        if st & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(DriverError::IoError);
        }

        Ok(())
    }

}

impl Driver for AtaPrimaryMaster {
    fn name(&self) -> &'static str {
        "ata0-master"
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Block
    }

    fn init(&self) -> Result<(), DriverError> {
        match self.issue_identify() {
            Ok(()) => {
                klog!("[ata] primary master ready\n");
                Ok(())
            }
            Err(err) => {
                klog!("[ata] identify failed: {:?}\n", err);
                Err(err)
            }
        }
    }
}

impl BlockDevice for AtaPrimaryMaster {
    fn block_size(&self) -> usize {
        SECTOR_BYTES
    }

    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), DriverError> {
        if buf.len() % SECTOR_BYTES != 0 {
            return Err(DriverError::Unsupported);
        }

        let sectors = buf.len() / SECTOR_BYTES;
        if sectors == 0 {
            return Ok(());
        }

        for (index, chunk) in buf.chunks_mut(SECTOR_BYTES).enumerate() {
            let mut sector = [0u8; SECTOR_BYTES];
            self.pio_read_sector(lba + index as u64, &mut sector)?;
            chunk.copy_from_slice(&sector);
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), DriverError> {
        unsafe {
            outb(self.io_base() + REG_COMMAND, CMD_CACHE_FLUSH);
        }

        // Wait until BSY=0; ERR/DF clear
        self.wait_until(0, 0, 200_000)?;

        let st = unsafe { inb(self.io_base() + REG_STATUS) };

        if st & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(DriverError::IoError);
        }

        Ok(())
    }

    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), DriverError> {
        if buf.len() % SECTOR_BYTES != 0 {
            return Err(DriverError::Unsupported);
        }
        let sectors = buf.len() / SECTOR_BYTES;
        if sectors == 0 { return Ok(()); }

        for (i, chunk) in buf.chunks(SECTOR_BYTES).enumerate() {
            // SAFETY: chunk is exactly 512 bytes
            let mut sector = [0u8; SECTOR_BYTES];
            sector.copy_from_slice(chunk);
            self.pio_write_sector(lba + i as u64, &sector)?;
        }

        self.flush()?;

        Ok(())
    }

}

pub fn driver() -> &'static AtaPrimaryMaster {
    &ATA_PRIMARY
}
