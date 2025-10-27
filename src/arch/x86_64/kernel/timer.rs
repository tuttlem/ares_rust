#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::klog;
use crate::process;
use super::{interrupts, pit};

const DEFAULT_FREQUENCY_HZ: u32 = 100;
const PREEMPT_SLICE_TICKS: u64 = 1;

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

fn timer_handler(frame: &mut interrupts::InterruptFrame) {
    let tick = TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if tick % PREEMPT_SLICE_TICKS == 0 {
        // klog!("[timer] Prescaler tick: {}\n", tick);
        process::request_preempt(frame);
    }
}
