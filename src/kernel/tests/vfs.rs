#![cfg(kernel_test)]

use super::{TestCase, TestResult};
use crate::tests::common::TestBlockDevice;
use crate::vfs::ata::AtaScratchFile;
use crate::vfs::{VfsError, VfsFile};

const BLOCK_SIZE: usize = 512;
static SCRATCH_DEVICE: TestBlockDevice<{ BLOCK_SIZE * 4 }> =
    TestBlockDevice::new("test-scratch", BLOCK_SIZE);

pub const TESTS: &[TestCase] = &[
    TestCase::new("vfs.scratch_roundtrip", scratch_roundtrip),
    TestCase::new("vfs.scratch_overlap", scratch_overlap),
    TestCase::new("vfs.scratch_bounds", scratch_bounds),
];

fn scratch_roundtrip() -> TestResult {
    SCRATCH_DEVICE.reset();
    let file = AtaScratchFile::new(&SCRATCH_DEVICE, 0, "scratch");
    let payload = b"kernel";
    let written = file.write_at(0, payload).map_err(|_| "scratch write failed")?;
    if written != payload.len() {
        return Err("scratch short write");
    }

    let mut buf = [0u8; 6];
    let read = file.read_at(0, &mut buf).map_err(|_| "scratch read failed")?;
    if read != payload.len() || buf != *payload {
        return Err("scratch roundtrip mismatch");
    }
    Ok(())
}

fn scratch_overlap() -> TestResult {
    SCRATCH_DEVICE.reset();
    let file = AtaScratchFile::new(&SCRATCH_DEVICE, 0, "scratch");

    file.write_at(100, b"abcdef").map_err(|_| "initial write failed")?;
    file.write_at(102, b"XYZ").map_err(|_| "overlap write failed")?;

    let mut buf = [0u8; 6];
    file.read_at(100, &mut buf).map_err(|_| "overlap read failed")?;
    if &buf != b"abXYZf" {
        return Err("overlap result mismatch");
    }
    Ok(())
}

fn scratch_bounds() -> TestResult {
    SCRATCH_DEVICE.reset();
    let file = AtaScratchFile::new(&SCRATCH_DEVICE, 0, "scratch");
    let buf = [0u8; 1];
    match file.write_at(BLOCK_SIZE as u64, &buf) {
        Err(VfsError::InvalidOffset) => {}
        _ => return Err("expected invalid offset error"),
    }

    match file.read_at(BLOCK_SIZE as u64, &mut [0u8; 1]) {
        Err(VfsError::InvalidOffset) => Ok(()),
        _ => Err("expected invalid offset error"),
    }
}
