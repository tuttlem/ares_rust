use ares_core::drivers::mock::MemBlockDevice;
use ares_core::fs::fat::{self, FatError};

fn fat_image_with_hello() -> Vec<u8> {
    const SECTOR_SIZE: usize = 512;
    let mut image = vec![0u8; SECTOR_SIZE * 10];

    // BIOS Parameter Block for FAT16
    {
        let bpb = &mut image[0..SECTOR_SIZE];
        bpb[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
        bpb[13] = 1; // sectors per cluster
        bpb[14..16].copy_from_slice(&(1u16).to_le_bytes()); // reserved sectors
        bpb[16] = 1; // number of FATs
        bpb[17..19].copy_from_slice(&(16u16).to_le_bytes()); // root entries
        bpb[21] = 0xF8; // media descriptor
        bpb[22..24].copy_from_slice(&(1u16).to_le_bytes()); // sectors per FAT
        bpb[24..26].copy_from_slice(&(1u16).to_le_bytes()); // sectors per track
        bpb[26..28].copy_from_slice(&(1u16).to_le_bytes()); // heads
        bpb[510] = 0x55;
        bpb[511] = 0xAA;
    }

    // FAT table (sector 1)
    {
        let fat_sector = &mut image[SECTOR_SIZE..SECTOR_SIZE * 2];
        fat_sector[0] = 0xF8;
        fat_sector[1] = 0xFF;
        fat_sector[2] = 0xFF;
        fat_sector[3] = 0xFF;
    }

    // Root directory (sector 2)
    {
        let root = &mut image[SECTOR_SIZE * 2..SECTOR_SIZE * 3];
        root[0..11].copy_from_slice(b"HELLO   TXT");
        root[11] = 0x20; // archive attribute
        root[26..28].copy_from_slice(&(2u16).to_le_bytes());
        root[28..32].copy_from_slice(&(5u32).to_le_bytes());
    }

    // Data cluster (cluster 2 -> sector 3)
    {
        let data = &mut image[SECTOR_SIZE * 3..SECTOR_SIZE * 4];
        data[..5].copy_from_slice(b"Hello");
    }

    image
}

#[test]
fn short_name_conversion() {
    let short = fat::test_format_short_name("HELLO.TXT").unwrap();
    assert_eq!(&short, b"HELLO   TXT");
    assert!(fat::test_format_short_name("too_long_name.ext").is_none());
}

#[test]
fn open_and_read_file() {
    let image = fat_image_with_hello();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, 512)));
    fat::mount(dev, 0).expect("mount");
    let file = fat::open_file("HELLO.TXT").expect("open");
    let mut buf = [0u8; 8];
    let read = file.read_at(0, &mut buf).expect("read");
    assert_eq!(read, 5);
    assert_eq!(&buf[..5], b"Hello");
}

#[test]
fn missing_file_errors() {
    let image = fat_image_with_hello();
    let dev = Box::leak(Box::new(MemBlockDevice::new("mem-fat", image, 512)));
    fat::mount(dev, 0).expect("mount");
    let result = fat::open_file("MISSING.TXT");
    assert!(matches!(result, Err(FatError::NotFound)));
}
