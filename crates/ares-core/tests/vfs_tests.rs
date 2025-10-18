use std::sync::Mutex;

use ares_core::drivers::mock::MemBlockDevice;
use ares_core::drivers::BlockDevice;
use ares_core::vfs::ata::AtaScratchFile;
use ares_core::vfs::{VfsError, VfsFile};

static SCRATCH_GUARD: Mutex<()> = Mutex::new(());
const BLOCK_SIZE: usize = 512;

fn fresh_device() -> &'static MemBlockDevice {
    let data = vec![0u8; BLOCK_SIZE * 4];
    Box::leak(Box::new(MemBlockDevice::new("scratch", data, BLOCK_SIZE)))
}

#[test]
fn scratch_read_write_roundtrip() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    let payload = b"kernel";
    assert_eq!(file.write_at(0, payload).unwrap(), payload.len());

    let mut buf = [0u8; 6];
    assert_eq!(file.read_at(0, &mut buf).unwrap(), payload.len());
    assert_eq!(&buf, payload);

    assert_eq!(file.size().unwrap(), BLOCK_SIZE as u64);
}

#[test]
fn scratch_bounds_checks() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    let mut single = [0u8; 1];
    let err = file.read_at(BLOCK_SIZE as u64, &mut single).unwrap_err();
    assert_eq!(err, VfsError::InvalidOffset);

    let err = file.write_at(BLOCK_SIZE as u64, &single).unwrap_err();
    assert_eq!(err, VfsError::InvalidOffset);
}

#[test]
fn scratch_overlapping_writes() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    file.write_at(100, b"abcdef").unwrap();
    file.write_at(102, b"XYZ").unwrap();

    let mut buf = [0u8; 6];
    file.read_at(100, &mut buf).unwrap();
    assert_eq!(&buf, b"abXYZf");

    let mut disk = [0u8; BLOCK_SIZE];
    dev.read_blocks(0, &mut disk).unwrap();
    assert_eq!(&disk[100..106], b"abXYZf");
}

#[test]
fn scratch_rejects_large_write() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    let buf = [0u8; BLOCK_SIZE + 16];
    let err = file.write_at(0, &buf).unwrap_err();
    assert_eq!(err, VfsError::Unsupported);
}

#[test]
fn scratch_seek_write_within_sector() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    file.write_at(10, b"abc").unwrap();

    let mut buf = [0u8; 3];
    file.read_at(10, &mut buf).unwrap();
    assert_eq!(&buf, b"abc");

    let mut whole = [0u8; BLOCK_SIZE];
    dev.read_blocks(0, &mut whole).unwrap();
    assert_eq!(&whole[10..13], b"abc");
}

#[test]
fn scratch_overflow_is_unsupported() {
    let _guard = SCRATCH_GUARD.lock().unwrap();
    let dev = fresh_device();
    let file = unsafe { AtaScratchFile::init(dev, 0, "scratch") };

    let big = [0u8; BLOCK_SIZE + 1];
    let err = file.write_at(0, &big).unwrap_err();
    assert_eq!(err, VfsError::Unsupported);
}
