#![cfg(kernel_test)]

use super::{TestCase, TestResult};
use crate::mem::heap::{self, HeapBox};

pub const TESTS: &[TestCase] = &[TestCase::new("memory.heap_allocation", heap_allocation)];

fn heap_allocation() -> TestResult {
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
