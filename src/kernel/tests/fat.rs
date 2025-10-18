#![cfg(kernel_test)]

use super::{TestCase, TestResult};
use crate::tests::common::{mount_hello};

pub const TESTS: &[TestCase] = &[
    TestCase::new("fat.read_hello", read_hello),
    TestCase::new("fat.read_beyond_end", read_beyond_end),
];

fn read_hello() -> TestResult {
    mount_hello()?;
    let file = crate::fs::fat::open_file("HELLO.TXT").map_err(|_| "open HELLO failed")?;
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
    let file = crate::fs::fat::open_file("HELLO.TXT").map_err(|_| "open HELLO failed")?;
    let mut buf = [0u8; 16];
    let count = file
        .read_at(1024, &mut buf)
        .map_err(|_| "read past end failed")?;
    if count != 0 {
        return Err("expected eof");
    }
    Ok(())
}
