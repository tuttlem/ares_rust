#![cfg(kernel_test)]

use core::hint::spin_loop;

use crate::arch::x86_64::qemu;
use crate::klog;
use crate::mem::heap::{self, HeapBox};
use crate::process;

type TestResult = Result<(), &'static str>;

type TestCase = (&'static str, fn() -> TestResult);

const TESTS: &[TestCase] = &[
    ("heap_allocation", test_heap_allocation),
    ("process_spawn_snapshot", test_process_spawn_snapshot),
];

pub fn run() -> ! {
    klog!("[test] kernel test harness starting\n");
    let mut failures = 0u32;

    for (name, test) in TESTS {
        match test() {
            Ok(()) => klog!("[test] {name}: ok\n"),
            Err(msg) => {
                failures += 1;
                klog!("[test] {name}: FAIL ({msg})\n");
            }
        }
    }

    if failures == 0 {
        klog!("[test] all passed\n");
        qemu::exit_success();
    } else {
        klog!("[test] {failures} failure(s)\n");
        qemu::exit((failures as u8).max(1));
    }
}

fn test_heap_allocation() -> TestResult {
    let before = heap::remaining_bytes();
    {
        let mut boxed = HeapBox::new([0u64; 4]).map_err(|_| "heap alloc failed")?;
        for (i, slot) in boxed.iter_mut().enumerate() {
            *slot = i as u64;
        }
        if boxed[3] != 3 {
            return Err("heap contents corrupted");
        }
    }
    let after = heap::remaining_bytes();
    if after > before {
        return Err("heap reported more memory after free");
    }
    Ok(())
}

fn test_process_spawn_snapshot() -> TestResult {
    process::init().map_err(|_| "process init failed")?;

    extern "C" fn stub() -> ! {
        loop {
            spin_loop();
        }
    }

    let pid = process::spawn_kernel_process("test_task", stub).map_err(|_| "spawn failed")?;
    let snapshot = process::get_process(pid).ok_or("snapshot missing")?;
    if snapshot.name() != "test_task" {
        return Err("snapshot name mismatch");
    }
    if snapshot.pid() != pid {
        return Err("snapshot pid mismatch");
    }
    Ok(())
}
