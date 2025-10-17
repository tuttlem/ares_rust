# File System Support

The kernel now has a minimal FAT16-compatible filesystem module under
`src/kernel/fs/fat.rs`.  It is designed to plug into the existing VFS
interfaces so future filesystems can be swapped in with minimal
friction.

## Layout

- `fs/mod.rs` – declares filesystem modules.  Currently only `fat` is
  wired in.
- `fs/fat.rs` – implements a simple read-only FAT layer.  It reads the
  BIOS parameter block, locates FAT tables and the root directory, and
  exposes files as `VfsFile` objects.

## Mounting

`kmain` mounts the FAT volume during boot, right after the ATA scratch
file is initialised:

```rust
if let Some(ata_dev) = drivers::block_device_by_name("ata0-master") {
    unsafe {
        let file = AtaScratchFile::init(ata_dev, 2048, "ata0-scratch");
        klog!("[vfs] scratch file '{}' mounted at LBA {}", file.name(), 2048);
    }
    match fs::fat::mount(ata_dev, FAT_START_LBA) {
        Ok(()) => klog!("[fat] mounted volume at LBA {}", FAT_START_LBA),
        Err(err) => klog!("[fat] mount failed: {:?}", err),
    }
}
```

`FAT_START_LBA` is currently `4096`, so the filesystem must begin at
sector 4096 (2 MiB) inside the disk image.

## Access

To open a FAT file, use the normal syscall path with a `/fat/...`
prefix, for example `open("/fat/HELLO.TXT")`.  The path resolver maps
`/fat/` to the FAT module, while other names (`/scratch`, `/dev/null`,
etc.) continue to use their existing drivers.

Only 8.3 filenames in the root directory are supported right now.  The
module is read-only; `write_at` returns `VfsError::Unsupported`.

## Preparing a FAT image

Create a FAT16 volume starting at sector 4096 and copy files into it:

```bash
# format the FAT volume inside the raw disk image
mkfs.fat --offset=4096 -F 16 -n ARESFAT dist/x86_64/hda.img

# copy an 8.3 filename into the root directory
mcopy -i dist/x86_64/hda.img@@2097152 TEST.TXT ::TEST.TXT
```

`2097152` bytes = 4096 sectors × 512 bytes/sector.  Place your files in
the root directory using uppercase 8.3 names to match the current
parser.

A boot smoke test in `ticker_task_a` reads `/fat/HELLO.TXT` (if present)
and logs its contents, making it easy to confirm the filesystem is
mounted correctly.

Future work: directory traversal, write support, and a more flexible
VFS node hierarchy so multiple filesystem types can coexist.
