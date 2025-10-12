#![allow(dead_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

use crate::interrupts;
use crate::klog;
use crate::sync::spinlock::SpinLock;

const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB temporary heap

static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static HEAP: SpinLock<BumpAllocator> = SpinLock::new(BumpAllocator::new());

pub struct KernelAllocator;

struct BumpAllocator {
    start: usize,
    next: usize,
    end: usize,
    initialized: bool,
}

impl BumpAllocator {
    const fn new() -> Self {
        Self {
            start: 0,
            next: 0,
            end: 0,
            initialized: false,
        }
    }

    fn init(&mut self) -> bool {
        if !self.initialized {
            let base = core::ptr::addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
            self.start = base;
            self.next = base;
            self.end = base + HEAP_SIZE;
            self.initialized = true;
            true
        } else {
            false
        }
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if !self.initialized {
            self.init();
        }

        let align = layout.align().max(1);
        let size = layout.size().max(1);

        let alloc_start = align_up(self.next, align);
        let alloc_end = match alloc_start.checked_add(size) {
            Some(end) => end,
            None => return null_mut(),
        };

        if alloc_end > self.end {
            null_mut()
        } else {
            self.next = alloc_end;
            alloc_start as *mut u8
        }
    }

    fn remaining(&self) -> usize {
        if self.initialized {
            self.end.saturating_sub(self.next)
        } else {
            HEAP_SIZE
        }
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = HEAP.lock();
        let ptr = heap.alloc(layout);
        if ptr.is_null() {
            let remaining = heap.remaining();
            drop(heap);
            allocation_failed(layout, remaining);
        }
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator does not support deallocation
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = self.alloc(layout);
        if !ptr.is_null() {
            core::ptr::write_bytes(ptr, 0, layout.size());
        }
        ptr
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: KernelAllocator = KernelAllocator;

pub fn init() {
    let initialized_now = {
        let mut heap = HEAP.lock();
        heap.init()
    };

    if initialized_now {
        klog!("[heap] bump allocator ready ({} bytes)\n", HEAP_SIZE);
    }
}

fn allocation_failed(layout: Layout, remaining: usize) -> ! {
    interrupts::disable();
    klog!(
        "[heap] allocation failure: size={} align={} remaining={}\n",
        layout.size(),
        layout.align(),
        remaining
    );
    loop {
        core::hint::spin_loop();
    }
}

pub unsafe fn allocate(layout: Layout) -> *mut u8 {
    GLOBAL_ALLOCATOR.alloc(layout)
}
