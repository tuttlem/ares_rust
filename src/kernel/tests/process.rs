#![cfg(kernel_test)]

use core::hint::spin_loop;

use super::{TestCase, TestResult};
use crate::process::{self, AddressSpaceKind};
use crate::user;

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
    if snapshot.credentials().effective_uid() != user::ROOT_UID {
        return Err("snapshot uid mismatch");
    }
    if snapshot.credentials().effective_gid() != user::ROOT_GID {
        return Err("snapshot gid mismatch");
    }
    if snapshot.address_space().kind() != AddressSpaceKind::Kernel {
        return Err("snapshot address space mismatch");
    }
    if snapshot.user_stack().is_some() {
        return Err("kernel task should not have user stack");
    }
    if snapshot.user_entry().is_some() {
        return Err("kernel task should not have user entry");
    }
    Ok(())
}
