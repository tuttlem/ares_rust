#![cfg(kernel_test)]

use super::{TestCase, TestResult};
use crate::tests::common::TestBlockDevice;
use crate::fs::fat;

const SECTOR_SIZE: usize = 512;
static FAT_DEVICE: TestBlockDevice<{ SECTOR_SIZE * 12 }> =
    TestBlockDevice::new("test-fat", SECTOR_SIZE);

pub const TESTS: &[TestCase] = &[
    TestCase::new("fat.read_hello", read_hello),
    TestCase::new("fat.read_beyond_end", read_beyond_end),
];

fn read_hello() -> TestResult {
    mount_hello()?;
    let file = fat::open_file("HELLO.TXT").map_err(|_| "open HELLO failed")?;
    let mut buf = [0u8; 32];
    let count = file.read_at(0, &mut buf).map_err(|_| "read failed")?;
    if count == 0 {
        return Err("empty read");
    }
    let text = core::str::from_utf8(&buf[..count]).map_err(|_| "utf8 decode")?;
    if !text.starts_with("Hello") {
        return Err("unexpected contents");
    }
    Ok(())
}

fn read_beyond_end() -> TestResult {
    mount_hello()?;
    let file = fat::open_file("HELLO.TXT").map_err(|_| "open HELLO failed")?;
    let mut buf = [0u8; 16];
    let count = file.read_at(1024, &mut buf).map_err(|_| "read past end failed")?;
    if count != 0 {
        return Err("expected eof");
    }
    Ok(())
}

fn mount_hello() -> TestResult {
    let mut image = [0u8; SECTOR_SIZE * 10];

    {
        let bpb = &mut image[0..SECTOR_SIZE];
        bpb[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
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
        let fat = &mut image[SECTOR_SIZE..SECTOR_SIZE * 2];
        fat[0] = 0xF8;
        fat[1] = 0xFF;
        fat[2] = 0xFF;
        fat[3] = 0xFF;
        let cluster2 = 2 * 2;
        fat[cluster2..cluster2 + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    }

    {
        let root = &mut image[SECTOR_SIZE * 2..SECTOR_SIZE * 3];
        root[0..11].copy_from_slice(b"HELLO   TXT");
        root[11] = 0x20;
        root[26..28].copy_from_slice(&(2u16).to_le_bytes());
        root[28..32].copy_from_slice(&(5u32).to_le_bytes());
    }

    {
        let data = &mut image[SECTOR_SIZE * 3..SECTOR_SIZE * 4];
        data[..5].copy_from_slice(b"Hello");
    }

    FAT_DEVICE.reset();
    FAT_DEVICE
        .load_image(&image)
        .map_err(|_| "image too large")?;
    fat::mount(&FAT_DEVICE, 0).map_err(|_| "mount failed")?;
    Ok(())
}
