#![cfg(kernel_test)]

use core::hint::spin_loop;

use super::{TestCase, TestResult};
use crate::drivers;
use crate::process;
use crate::syscall;
use crate::tests::common::{init_scratch, mount_hello};
use crate::vfs::ata::AtaScratchFile;
use crate::vfs::{VfsError, VfsFile};

const BLOCK_SIZE: usize = 512;

pub const TESTS: &[TestCase] = &[
    TestCase::new("vfs.scratch_roundtrip", scratch_roundtrip),
    TestCase::new("vfs.scratch_overlap", scratch_overlap),
    TestCase::new("vfs.scratch_bounds", scratch_bounds),
    TestCase::new("vfs.scratch_stress", scratch_stress),
    TestCase::new("vfs.ticker_smoke", ticker_smoke_stress),
];

fn scratch_roundtrip() -> TestResult {
    init_scratch();
    let file = AtaScratchFile::get().ok_or("scratch not initialised")?;
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
    init_scratch();
    let file = AtaScratchFile::get().ok_or("scratch not initialised")?;

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
    init_scratch();
    let file = AtaScratchFile::get().ok_or("scratch not initialised")?;
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

fn scratch_stress() -> TestResult {
    init_scratch();
    let file = AtaScratchFile::get().ok_or("scratch not initialised")?;
    let mut buf = [0u8; 32];
    for i in 0..256u64 {
        let pattern = [i as u8; 32];
        let offset = (i * 7 % (BLOCK_SIZE as u64 - pattern.len() as u64)) as u64;
        file.write_at(offset, &pattern).map_err(|_| "stress write failed")?;
        buf.fill(0);
        file.read_at(offset, &mut buf).map_err(|_| "stress read failed")?;
        if buf != pattern {
            return Err("stress pattern mismatch");
        }
    }
    Ok(())
}

fn ticker_smoke_stress() -> TestResult {
    init_scratch();
    mount_hello()?;
    drivers::register_builtin();
    process::init().map_err(|_| "process init failed")?;

    const ITERATIONS: usize = 128;
    extern "C" fn dormant() -> ! {
        loop {
            spin_loop();
        }
    }

    let pid = process::spawn_kernel_process("vfs_syscall_ctx", dormant)
        .map_err(|_| "spawn syscall ctx failed")?;

    let result = (|| -> TestResult {
        process::set_current_pid(pid);
        for _ in 0..ITERATIONS {
            ticker_sequence()?;
        }
        Ok(())
    })();

    process::set_current_pid(0);
    result
}

fn ticker_sequence() -> Result<(), &'static str> {
    // /dev/null write
    let fd = syscall::open("/dev/null").map_err(|_| "open /dev/null")? as u64;
    syscall::write(fd, b"discard").map_err(|_| "write /dev/null")?;
    syscall::close(fd).map_err(|_| "close /dev/null")?;

    // /dev/zero read
    let fd = syscall::open("/dev/zero").map_err(|_| "open /dev/zero")? as u64;
    let mut buf = [0xAAu8; 16];
    let read = syscall::read(fd, &mut buf).map_err(|_| "read /dev/zero")?;
    if buf[..read].iter().any(|&b| b != 0) {
        return Err("dev/zero data mismatch");
    }
    syscall::close(fd).map_err(|_| "close /dev/zero")?;

    // /scratch operations
    init_scratch();
    let fd = syscall::open("/scratch").map_err(|_| "open /scratch")? as u64;
    let data = b"seektest";
    syscall::write(fd, data).map_err(|_| "write /scratch")?;
    syscall::seek(fd, 2, syscall::SeekWhence::Set).map_err(|_| "seek /scratch")?;
    let mut scratch_buf = [0u8; 6];
    let read = syscall::read(fd, &mut scratch_buf).map_err(|_| "read /scratch")?;
    if &scratch_buf[..read] != b"ektest" {
        return Err("scratch mismatch");
    }
    syscall::close(fd).map_err(|_| "close /scratch")?;

    // /fat/HELLO.TXT
    mount_hello()?;
    let fd = syscall::open("/fat/HELLO.TXT").map_err(|_| "open /fat")? as u64;
    let mut fat_buf = [0u8; 32];
    let read = syscall::read(fd, &mut fat_buf).map_err(|_| "read /fat")?;
    if !core::str::from_utf8(&fat_buf[..read]).map_or(false, |s| s.starts_with("Hello")) {
        return Err("fat content mismatch");
    }
    syscall::close(fd).map_err(|_| "close /fat")?;

    Ok(())
}
