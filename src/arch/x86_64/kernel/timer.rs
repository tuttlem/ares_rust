#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::klog;
use super::{interrupts, pit};

const DEFAULT_FREQUENCY_HZ: u32 = 100;

static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static FREQUENCY_HZ: AtomicU32 = AtomicU32::new(0);

pub fn init() {
    init_with_frequency(DEFAULT_FREQUENCY_HZ);
}

pub fn init_with_frequency(hz: u32) {
    FREQUENCY_HZ.store(hz, Ordering::Relaxed);
    interrupts::register_handler(interrupts::vectors::PIT, timer_handler);
    interrupts::enable_vector(interrupts::vectors::PIT);
    pit::init_frequency(hz);
    klog!("[timer] PIT set to {} Hz\n", hz);
}

pub fn ticks() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

fn timer_handler(_frame: &mut interrupts::InterruptFrame) {
    TICK_COUNT.fetch_add(1, Ordering::Relaxed);
}
