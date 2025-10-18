#![cfg(kernel_test)]

use core::hint::spin_loop;

use super::{TestCase, TestResult};
use crate::process;

pub const TESTS: &[TestCase] = &[TestCase::new("process.spawn_snapshot", spawn_snapshot)];

fn spawn_snapshot() -> TestResult {
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
