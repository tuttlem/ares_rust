use alloc::vec;
use alloc::vec::Vec;

use crate::fs::fat;
use crate::vfs::VfsError;

#[derive(Debug)]
pub enum FileError {
    NotFound,
    Io,
}

pub fn read_binary(path: &str) -> Result<Vec<u8>, FileError> {
    let trimmed = path.strip_prefix("/bin/").ok_or(FileError::NotFound)?;
    crate::klog!("[userfs] read_binary trimmed='{}'\n", trimmed);

    let file = fat::open_file(trimmed).map_err(|err| {
        crate::klog!("[userfs] open_file error {:?}\n", err);
        match err {
            fat::FatError::NotFound | fat::FatError::InvalidPath => FileError::NotFound,
            _ => FileError::Io,
        }
    })?;
    crate::klog!("[userfs] open_file ok\n");

    let size = file.size().map_err(map_vfs_err)? as usize;
    crate::klog!("[userfs] file size={} bytes\n", size);
    let mut buffer = vec![0u8; size];
    if size > 0 {
        crate::klog!("[userfs] read_at request size={}\n", buffer.len());
        let read = file.read_at(0, &mut buffer).map_err(|err| {
            crate::klog!("[userfs] read_at error {:?}\n", err);
            map_vfs_err(err)
        })?;
        crate::klog!("[userfs] read_at returned {} bytes\n", read);
        if read != size {
            buffer.truncate(read);
        }
    }
    Ok(buffer)
}

fn map_vfs_err(err: VfsError) -> FileError {
    match err {
        VfsError::Io => FileError::Io,
        _ => FileError::NotFound,
    }
}
