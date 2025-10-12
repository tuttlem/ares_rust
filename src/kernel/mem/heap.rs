#![allow(dead_code)]

use core::alloc::Layout;
use core::mem::{align_of, size_of};
use core::ptr::{self, null_mut, NonNull};
use core::ops::{Deref, DerefMut};

use crate::interrupts;
use crate::klog;
use crate::sync::spinlock::SpinLock;

const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB temporary heap

static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static ALLOCATOR: SpinLock<LinkedListAllocator> = SpinLock::new(LinkedListAllocator::new());

pub struct KernelAllocator;

struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }

    unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.head.next = None;
        self.insert_region(heap_start, heap_size);
    }

    fn min_region_size() -> usize {
        size_of::<ListNode>()
    }

    fn remaining(&self) -> usize {
        let mut total = 0;
        let mut current = &self.head;
        while let Some(node) = current.next.as_deref() {
            total += node.size;
            current = node;
        }
        total
    }

    unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(Self::min_region_size());
        let align = layout.align().max(align_of::<ListNode>());

        let mut current = &mut self.head;
        while let Some(region) = current.next.as_mut() {
            let alloc_start = align_up(region.start_addr(), align);
            let alloc_end = match alloc_start.checked_add(size) {
                Some(end) => end,
                None => return null_mut(),
            };

            if alloc_end > region.end_addr() {
                current = current.next.as_mut().unwrap();
                continue;
            }

            let next = region.next.take();
            let region_start = region.start_addr();
            let region_size = region.size;

            current.next = next;

            let excess_before = alloc_start - region_start;
            if excess_before >= Self::min_region_size() {
                self.insert_region(region_start, excess_before);
            }

            let excess_after = region_start + region_size - alloc_end;
            if excess_after >= Self::min_region_size() {
                self.insert_region(alloc_end, excess_after);
            }

            return alloc_start as *mut u8;
        }

        null_mut()
    }

    unsafe fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(Self::min_region_size());
        self.insert_region(ptr as usize, size);
    }

    unsafe fn insert_region(&mut self, addr: usize, size: usize) {
        let align = align_of::<ListNode>();
        let start = align_up(addr, align);
        let end = match addr.checked_add(size) {
            Some(end) => end,
            None => return,
        };

        if start >= end {
            return;
        }

        let size = end - start;
        if size < Self::min_region_size() {
            return;
        }

        let mut current = &mut self.head;
        while let Some(next) = current.next.as_ref() {
            if next.start_addr() >= start {
                break;
            }
            current = current.next.as_mut().unwrap();
        }

        let mut node = ListNode::new(size);
        node.next = current.next.take();

        let node_ptr = start as *mut ListNode;
        node_ptr.write(node);
        current.next = Some(&mut *node_ptr);

        self.merge_with_next(node_ptr);
        self.merge_with_previous(node_ptr);
    }

    unsafe fn merge_with_next(&mut self, node_ptr: *mut ListNode) {
        let node = &mut *node_ptr;
        loop {
            let node_end = node.end_addr();
            let next = match node.next.as_mut() {
                Some(next) => next,
                None => break,
            };

            if node_end != next.start_addr() {
                break;
            }

            let next_next = next.next.take();
            node.size += next.size;
            node.next = next_next;
        }
    }

    unsafe fn merge_with_previous(&mut self, node_ptr: *mut ListNode) {
        let mut current = &mut self.head;
        while let Some(next) = current.next.as_mut() {
            let next_ptr = &mut **next as *mut ListNode;
            if next_ptr == node_ptr {
                if current.size != 0 && current.end_addr() == (*node_ptr).start_addr() {
                    let node = &mut *node_ptr;
                    let next_next = node.next.take();
                    current.size += node.size;
                    current.next = next_next;
                    let current_ptr = current as *mut ListNode;
                    self.merge_with_next(current_ptr);
                }
                break;
            }
            current = current.next.as_mut().unwrap();
        }
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
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

pub fn init() {
    let heap_start = core::ptr::addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
    let heap_size = HEAP_SIZE;
    unsafe {
        ALLOCATOR.lock().init(heap_start, heap_size);
    }
    klog!("[heap] allocator ready ({} bytes)\n", HEAP_SIZE);
}

pub fn remaining_bytes() -> usize {
    let allocator = ALLOCATOR.lock();
    allocator.remaining()
}

pub unsafe fn allocate(layout: Layout) -> *mut u8 {
    ALLOCATOR.lock().allocate(layout)
}

pub unsafe fn deallocate(ptr: *mut u8, layout: Layout) {
    ALLOCATOR.lock().deallocate(ptr, layout)
}

pub struct HeapBox<T> {
    ptr: NonNull<T>,
    layout: Layout,
}

impl<T> HeapBox<T> {
    pub fn new(value: T) -> Result<Self, ()> {
        let layout = Layout::new::<T>();
        let raw = unsafe { allocate(layout) } as *mut T;
        if raw.is_null() {
            return Err(());
        }
        unsafe { raw.write(value); }
        Ok(Self {
            ptr: unsafe { NonNull::new_unchecked(raw) },
            layout,
        })
    }
}

impl<T> Deref for HeapBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> DerefMut for HeapBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T> Drop for HeapBox<T> {
    fn drop(&mut self) {
        unsafe {
            ptr::drop_in_place(self.ptr.as_ptr());
            deallocate(self.ptr.as_ptr() as *mut u8, self.layout);
        }
    }
}
