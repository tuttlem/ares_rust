#![allow(dead_code)]

use crate::drivers::BlockDevice;
use crate::klog;
use crate::sync::spinlock::SpinLock;
use crate::vfs::{VfsError, VfsFile, VfsResult};

use crate::mem::heap;
use core::alloc::Layout;

use core::cmp;

const SECTOR_SIZE: usize = 512;
const SHORT_NAME_LEN: usize = 11;
const FAT16_END: u16 = 0xFFF8;

#[derive(Debug, Copy, Clone)]
pub enum FatError {
    NotMounted,
    InvalidPath,
    NotFound,
    Io,
}

struct FatVolume {
    device: &'static dyn BlockDevice,
    start_lba: u64,
    bytes_per_sector: usize,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entries: u16,
    sectors_per_fat: u16,
    fat_lba: u64,
    root_dir_lba: u64,
    root_dir_sectors: u32,
    data_lba: u64,
    bytes_per_cluster: usize,
}

impl FatVolume {
    fn load(device: &'static dyn BlockDevice, start_lba: u64) -> Result<Self, FatError> {
        let mut sector = [0u8; SECTOR_SIZE];
        device
            .read_blocks(start_lba, &mut sector)
            .map_err(|_| FatError::Io)?;

        let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]) as usize;
        if bytes_per_sector != SECTOR_SIZE {
            return Err(FatError::Io);
        }

        let sectors_per_cluster = sector[13];
        let reserved_sectors = u16::from_le_bytes([sector[14], sector[15]]);
        let num_fats = sector[16];
        let root_entries = u16::from_le_bytes([sector[17], sector[18]]);
        let sectors_per_fat = u16::from_le_bytes([sector[22], sector[23]]);

        let fat_lba = start_lba + reserved_sectors as u64;
        let root_dir_lba = fat_lba + (num_fats as u64 * sectors_per_fat as u64);
        let root_dir_sectors = ((root_entries as u32 * 32) + (bytes_per_sector as u32 - 1)) / bytes_per_sector as u32;
        let data_lba = root_dir_lba + root_dir_sectors as u64;

        Ok(Self {
            device,
            start_lba,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            root_entries,
            sectors_per_fat,
            fat_lba,
            root_dir_lba,
            root_dir_sectors,
            data_lba,
            bytes_per_cluster: bytes_per_sector * sectors_per_cluster as usize,
        })
    }

    fn read_sector(&self, lba: u64, buffer: &mut [u8; SECTOR_SIZE]) -> Result<(), FatError> {
        self.device
            .read_blocks(lba, buffer)
            .map_err(|_| FatError::Io)
    }

    fn cluster_to_lba(&self, cluster: u16) -> u64 {
        self.data_lba + ((cluster as u64 - 2) * self.sectors_per_cluster as u64)
    }

    fn next_cluster(&self, cluster: u16) -> Result<Option<u16>, FatError> {
        let fat_offset = cluster as usize * 2;
        let fat_sector = fat_offset / self.bytes_per_sector;
        let offset_within = fat_offset % self.bytes_per_sector;

        let mut sector = [0u8; SECTOR_SIZE];
        let fat_lba = self.fat_lba + fat_sector as u64;
        self.read_sector(fat_lba, &mut sector)?;

        let entry = u16::from_le_bytes([
            sector[offset_within],
            sector[offset_within + 1],
        ]);

        if entry >= FAT16_END {
            Ok(None)
        } else {
            Ok(Some(entry))
        }
    }

    fn cluster_for_offset(&self, start_cluster: u16, mut offset: u64) -> Result<Option<(u16, u64)>, FatError> {
        if start_cluster == 0 {
            return Ok(None);
        }

        let cluster_bytes = self.bytes_per_cluster as u64;
        let mut cluster = start_cluster;
        while offset >= cluster_bytes {
            match self.next_cluster(cluster)? {
                Some(next) => {
                    cluster = next;
                    offset -= cluster_bytes;
                }
                None => return Ok(None),
            }
        }
        Ok(Some((cluster, offset)))
    }

    fn read_cluster_slice(
        &self,
        cluster: u16,
        offset: usize,
        dest: &mut [u8],
    ) -> Result<(), FatError> {
        let mut remaining = dest.len();
        let mut dest_offset = 0;
        let mut cluster_offset = offset;
        let bytes_per_sector = self.bytes_per_sector;
        let sectors_per_cluster = self.sectors_per_cluster as usize;

        for sector_index in cluster_offset / bytes_per_sector..sectors_per_cluster {
            if remaining == 0 {
                break;
            }
            let mut sector = [0u8; SECTOR_SIZE];
            let lba = self.cluster_to_lba(cluster) + sector_index as u64;
            self.read_sector(lba, &mut sector)?;

            let within_sector = if sector_index == (cluster_offset / bytes_per_sector) {
                cluster_offset % bytes_per_sector
            } else {
                0
            };

            let copy = cmp::min(bytes_per_sector - within_sector, remaining);
            dest[dest_offset..dest_offset + copy]
                .copy_from_slice(&sector[within_sector..within_sector + copy]);
            dest_offset += copy;
            remaining -= copy;
            cluster_offset = 0;
        }

        Ok(())
    }

    fn find_root_file(&self, path: &str) -> Result<(u16, u32), FatError> {
        let short_name = format_short_name(path).ok_or(FatError::InvalidPath)?;
        let entries_per_sector = self.bytes_per_sector / 32;
        let mut sector_buffer = [0u8; SECTOR_SIZE];

        for sector_index in 0..self.root_dir_sectors {
            let lba = self.root_dir_lba + sector_index as u64;
            self.read_sector(lba, &mut sector_buffer)?;

            for entry_index in 0..entries_per_sector {
                let offset = entry_index * 32;
                let entry = &sector_buffer[offset..offset + 32];
                let first = entry[0];
                if first == 0x00 {
                    return Err(FatError::NotFound);
                }
                if first == 0xE5 || entry[11] == 0x0F {
                    continue;
                }
                if entry[11] & 0x08 != 0 || entry[11] & 0x10 != 0 {
                    continue;
                }
                if entry[..SHORT_NAME_LEN] != short_name {
                    continue;
                }

                let start_cluster = u16::from_le_bytes([entry[26], entry[27]]);
                let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);
                return Ok((start_cluster, size));
            }
        }

        Err(FatError::NotFound)
    }
}

pub struct FatFile {
    volume: &'static FatVolume,
    start_cluster: u16,
    size: u32,
}

impl VfsFile for FatFile {
    fn name(&self) -> &'static str {
        "fat-file"
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if offset >= self.size as u64 {
            return Ok(0);
        }

        let remaining_file = (self.size as u64 - offset) as usize;
        let mut total = cmp::min(buf.len(), remaining_file);
        let mut written = 0;
        let mut current_offset = offset;

        while total > 0 {
            let (cluster, offset_in_cluster) = match self
                .volume
                .cluster_for_offset(self.start_cluster, current_offset)
            {
                Ok(Some(info)) => info,
                Ok(None) => break,
                Err(_) => return Err(VfsError::Io),
            };

            let cluster_remaining = self.volume.bytes_per_cluster as u64 - offset_in_cluster;
            let to_copy = cmp::min(cluster_remaining as usize, total);
            if let Err(_) = self.volume.read_cluster_slice(
                cluster,
                offset_in_cluster as usize,
                &mut buf[written..written + to_copy],
            ) {
                return Err(VfsError::Io);
            }

            written += to_copy;
            total -= to_copy;
            current_offset += to_copy as u64;
        }

        Ok(written)
    }

    fn write_at(&self, _offset: u64, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }

    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<u64> {
        Ok(self.size as u64)
    }
}

static FAT_VOLUME: SpinLock<Option<FatVolume>> = SpinLock::new(None);

pub fn mount(device: &'static dyn BlockDevice, start_lba: u64) -> Result<(), FatError> {
    let volume = FatVolume::load(device, start_lba)?;
    let mut slot = FAT_VOLUME.lock();
    *slot = Some(volume);
    klog!("[fat] mounted at LBA {}\n", start_lba);
    Ok(())
}

pub fn open_file(path: &str) -> Result<&'static dyn VfsFile, FatError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(FatError::InvalidPath);
    }

    let (volume_ptr, entry) = {
        let guard = FAT_VOLUME.lock();
        let volume = guard.as_ref().ok_or(FatError::NotMounted)?;
        let info = volume.find_root_file(trimmed)?;
        (volume as *const FatVolume, info)
    };

    let volume_ref = unsafe { &*volume_ptr };
    let file = FatFile {
        volume: volume_ref,
        start_cluster: entry.0,
        size: entry.1,
    };

    let layout = Layout::new::<FatFile>();
    let raw = unsafe { heap::allocate(layout) } as *mut FatFile;
    if raw.is_null() {
        return Err(FatError::Io);
    }
    unsafe {
        raw.write(file);
        Ok(&*raw)
    }
}

fn format_short_name(path: &str) -> Option<[u8; SHORT_NAME_LEN]> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains('/') {
        return None;
    }

    let mut parts = trimmed.split('.');
    let name_part = parts.next()?;
    let ext_part = parts.next();
    if parts.next().is_some() {
        return None;
    }

    if name_part.is_empty() || name_part.len() > 8 {
        return None;
    }
    if let Some(ext) = ext_part {
        if ext.len() > 3 {
            return None;
        }
    }

    let mut short = [b' '; SHORT_NAME_LEN];
    for (i, ch) in name_part.chars().enumerate() {
        short[i] = to_short_char(ch)?;
    }

    if let Some(ext) = ext_part {
        for (i, ch) in ext.chars().enumerate() {
            short[8 + i] = to_short_char(ch)?;
        }
    }

    Some(short)
}

fn to_short_char(ch: char) -> Option<u8> {
    if ch.is_ascii_lowercase() {
        Some(ch.to_ascii_uppercase() as u8)
    } else if ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_' {
        Some(ch as u8)
    } else {
        None
    }
}
