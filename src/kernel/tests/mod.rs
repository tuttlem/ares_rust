#![cfg(kernel_test)]

use crate::arch::x86_64::qemu;
use crate::klog;

mod common;
mod memory;
mod process;
mod vfs;
mod fat;

pub type TestResult = Result<(), &'static str>;

#[derive(Copy, Clone)]
pub struct TestCase {
    pub name: &'static str,
    pub func: fn() -> TestResult,
}

impl TestCase {
    pub const fn new(name: &'static str, func: fn() -> TestResult) -> Self {
        Self { name, func }
    }

    fn run(self) -> TestResult {
        (self.func)()
    }
}

const SUITES: &[(&str, &[TestCase])] = &[
    ("memory", memory::TESTS),
    ("process", process::TESTS),
    ("vfs", vfs::TESTS),
    ("fat", fat::TESTS),
];

pub fn run(multiboot_info_addr: usize) -> ! {
    let filter = unsafe { command_line_filter(multiboot_info_addr) };

    match filter {
        Some(f) => klog!("[test] kernel test harness starting (filter='{f}')\n"),
        None => klog!("[test] kernel test harness starting\n"),
    }

    let mut failures = 0u32;
    let mut executed = 0u32;

    for case in all_cases() {
        if !should_run(case.name, filter) {
            continue;
        }
        executed += 1;
        match case.run() {
            Ok(()) => klog!("[test] {}: ok\n", case.name),
            Err(msg) => {
                failures += 1;
                klog!("[test] {}: FAIL ({})\n", case.name, msg);
            }
        }
    }

    if executed == 0 {
        if let Some(f) = filter {
            klog!("[test] no tests matched filter '{f}'\n");
        } else {
            klog!("[test] no tests registered\n");
        }
    }

    if failures == 0 {
        klog!("[test] all passed\n");
        qemu::exit_success();
    } else {
        klog!("[test] {} failure(s)\n", failures);
        qemu::exit((failures as u8).max(1));
    }
}

fn all_cases() -> impl Iterator<Item = TestCase> {
    SUITES.iter().flat_map(|(_, cases)| cases.iter().copied())
}

fn should_run(name: &str, filter: Option<&'static str>) -> bool {
    match filter {
        None => true,
        Some(prefix) => name.starts_with(prefix),
    }
}

unsafe fn command_line_filter(multiboot_info_addr: usize) -> Option<&'static str> {
    let cmdline = parse_cmdline(multiboot_info_addr)?;
    extract_filter(cmdline)
}

fn extract_filter(cmdline: &'static str) -> Option<&'static str> {
    for part in cmdline.split_ascii_whitespace() {
        if let Some(value) = part.strip_prefix("test=") {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

unsafe fn parse_cmdline(multiboot_info_addr: usize) -> Option<&'static str> {
    const TAG_TYPE_END: u32 = 0;
    const TAG_TYPE_CMDLINE: u32 = 1;

    let total_size = *(multiboot_info_addr as *const u32) as usize;
    let mut current = multiboot_info_addr + core::mem::size_of::<u32>() * 2;
    let end = multiboot_info_addr + total_size;

    while current < end {
        let header = &*(current as *const TagHeader);
        if header.tag_type == TAG_TYPE_END {
            break;
        }
        if header.tag_type == TAG_TYPE_CMDLINE {
            let data_ptr = current + core::mem::size_of::<TagHeader>();
            let len = header.size as usize - core::mem::size_of::<TagHeader>();
            if len == 0 {
                return None;
            }
            let bytes = core::slice::from_raw_parts(data_ptr as *const u8, len);
            let terminator = bytes.iter().position(|&b| b == 0).unwrap_or(len);
            if terminator == 0 {
                return None;
            }
            let slice = &bytes[..terminator];
            return core::str::from_utf8(slice).ok();
        }
        current = align_up(current + header.size as usize, 8);
    }
    None
}

fn align_up(value: usize, align: usize) -> usize {
    let mask = align - 1;
    (value + mask) & !mask
}

#[repr(C)]
struct TagHeader {
    tag_type: u32,
    size: u32,
}
