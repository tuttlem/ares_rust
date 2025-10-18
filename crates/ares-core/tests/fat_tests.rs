use std::sync::Mutex;

use ares_core::drivers::mock::MemBlockDevice;
use ares_core::fs::fat::{self, FatError};

const SECTOR_SIZE: usize = 512;
static FAT_GUARD: Mutex<()> = Mutex::new(());

fn fat_image_with_hello() -> Vec<u8> {
    let mut image = vec![0u8; SECTOR_SIZE * 10];

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

    image
}

fn fat_image_with_large_file() -> Vec<u8> {
    let mut image = vec![0u8; SECTOR_SIZE * 12];

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
        let cluster3 = 3 * 2;
        fat[cluster3..cluster3 + 2].copy_from_slice(&4u16.to_le_bytes());
        let cluster4 = 4 * 2;
        fat[cluster4..cluster4 + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    }

    {
        let root = &mut image[SECTOR_SIZE * 2..SECTOR_SIZE * 3];
        root[0..11].copy_from_slice(b"HELLO   TXT");
        root[11] = 0x20;
        root[26..28].copy_from_slice(&(2u16).to_le_bytes());
        root[28..32].copy_from_slice(&(5u32).to_le_bytes());

        let entry = &mut root[32..64];
        entry[0..11].copy_from_slice(b"BIGFILE TXT");
        entry[11] = 0x20;
        entry[26..28].copy_from_slice(&(3u16).to_le_bytes());
        entry[28..32].copy_from_slice(&(600u32).to_le_bytes());
    }

    {
        let cluster2 = &mut image[SECTOR_SIZE * 3..SECTOR_SIZE * 4];
        cluster2.fill(0);
        cluster2[..5].copy_from_slice(b"Hello");
        let cluster3 = &mut image[SECTOR_SIZE * 4..SECTOR_SIZE * 5];
        cluster3.fill(b'A');
        let cluster4 = &mut image[SECTOR_SIZE * 5..SECTOR_SIZE * 6];
        cluster4.fill(b'B');
    }

    image
}

#[test]
fn short_name_conversion() {
    let _guard = FAT_GUARD.lock().unwrap();
    let short = fat::test_format_short_name("HELLO.TXT").unwrap();
    assert_eq!(&short, b"HELLO   TXT");
    assert!(fat::test_format_short_name("too_long_name.ext").is_none());
}

#[test]
fn open_and_read_file() {
    let _guard = FAT_GUARD.lock().unwrap();
    let image = fat_image_with_hello();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, SECTOR_SIZE)));
    fat::mount(dev, 0).expect("mount");
    let file = fat::open_file("HELLO.TXT").expect("open");
    let mut buf = [0u8; 8];
    let read = file.read_at(0, &mut buf).expect("read");
    assert_eq!(read, 5);
    assert_eq!(&buf[..5], b"Hello");
}

#[test]
fn missing_file_errors() {
    let _guard = FAT_GUARD.lock().unwrap();
    let image = fat_image_with_hello();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, SECTOR_SIZE)));
    fat::mount(dev, 0).expect("mount");
    let result = fat::open_file("MISSING.TXT");
    assert!(matches!(result, Err(FatError::NotFound)));
}

#[test]
fn read_beyond_end_returns_zero() {
    let _guard = FAT_GUARD.lock().unwrap();
    let image = fat_image_with_hello();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, SECTOR_SIZE)));
    fat::mount(dev, 0).expect("mount");
    let file = fat::open_file("HELLO.TXT").expect("open");
    let mut buf = [0u8; 32];
    let count = file.read_at(0, &mut buf).expect("read");
    assert_eq!(count, 5);
    let count = file.read_at(100, &mut buf).expect("read past end");
    assert_eq!(count, 0);
}

#[test]
fn multi_cluster_read() {
    let _guard = FAT_GUARD.lock().unwrap();
    let image = fat_image_with_large_file();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, SECTOR_SIZE)));
    fat::mount(dev, 0).expect("mount");
    let file = fat::open_file("BIGFILE.TXT").expect("open");
    let mut buf = [0u8; 700];
    let count = file.read_at(0, &mut buf).expect("read");
    assert_eq!(count, 600);
    assert!(buf[..512].iter().all(|&b| b == b'A'));
    assert!(buf[512..600].iter().all(|&b| b == b'B'));
}

#[test]
fn large_buffer_partial_read() {
    let _guard = FAT_GUARD.lock().unwrap();
    let image = fat_image_with_large_file();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, SECTOR_SIZE)));
    fat::mount(dev, 0).expect("mount");
    let file = fat::open_file("BIGFILE.TXT").expect("open");
    let mut buf = [0u8; 256];
    let count = file.read_at(256, &mut buf).expect("read slice");
    assert_eq!(count, 256);
    assert!(buf[..count].iter().all(|&b| b == b'A'));
}
