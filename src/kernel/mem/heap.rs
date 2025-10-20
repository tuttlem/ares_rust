#![allow(dead_code, non_snake_case)]

use core::alloc::Layout;
use core::mem::{align_of, size_of};
use core::ptr::{self, copy_nonoverlapping, null_mut, NonNull};
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

pub fn handle_alloc_error(layout: Layout) -> ! {
    let remaining = {
        let allocator = ALLOCATOR.lock();
        allocator.remaining()
    };
    allocation_failed(layout, remaining)
}

unsafe fn layout_from_size_align(size: usize, align: usize) -> Option<Layout> {
    Layout::from_size_align(size, align).ok()
}

#[export_name = "__rust_no_alloc_shim_is_unstable"]
pub unsafe extern "C" fn __rust_no_alloc_shim_is_unstable() {}

#[no_mangle]
pub unsafe extern "C" fn __rustc__rust_alloc(size: usize, align: usize) -> *mut u8 {
    __rust_alloc(size, align)
}

#[export_name = "__rustc::__rust_alloc"]
pub unsafe extern "C" fn __rustc_colon__rust_alloc(size: usize, align: usize) -> *mut u8 {
    __rust_alloc(size, align)
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc12___rust_alloc"]
pub unsafe extern "C" fn __rustc_mangled_alloc(size: usize, align: usize) -> *mut u8 {
    __rust_alloc(size, align)
}

#[no_mangle]
pub unsafe extern "C" fn __rust_alloc(size: usize, align: usize) -> *mut u8 {
    match layout_from_size_align(size, align) {
        Some(layout) => allocate(layout),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn __rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    match layout_from_size_align(size, align) {
        Some(layout) => {
            let ptr = allocate(layout);
            if !ptr.is_null() {
                ptr::write_bytes(ptr, 0, size);
            }
            ptr
        }
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn __rustc__rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    __rust_alloc_zeroed(size, align)
}

#[export_name = "__rustc::__rust_alloc_zeroed"]
pub unsafe extern "C" fn __rustc_colon__rust_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    __rust_alloc_zeroed(size, align)
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc19___rust_alloc_zeroed"]
pub unsafe extern "C" fn __rustc_mangled_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    __rust_alloc_zeroed(size, align)
}

#[no_mangle]
pub unsafe extern "C" fn __rust_dealloc(ptr: *mut u8, size: usize, align: usize) {
    if let Some(layout) = layout_from_size_align(size, align) {
        deallocate(ptr, layout);
    }
}

#[no_mangle]
pub unsafe extern "C" fn __rustc__rust_dealloc(ptr: *mut u8, size: usize, align: usize) {
    __rust_dealloc(ptr, size, align)
}

#[export_name = "__rustc::__rust_dealloc"]
pub unsafe extern "C" fn __rustc_colon__rust_dealloc(ptr: *mut u8, size: usize, align: usize) {
    __rust_dealloc(ptr, size, align)
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc14___rust_dealloc"]
pub unsafe extern "C" fn __rustc_mangled_dealloc(ptr: *mut u8, size: usize, align: usize) {
    __rust_dealloc(ptr, size, align)
}

#[no_mangle]
pub unsafe extern "C" fn __rust_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return __rust_alloc(new_size, align);
    }
    if new_size == 0 {
        __rust_dealloc(ptr, old_size, align);
        return core::ptr::null_mut();
    }

    let new_layout = match layout_from_size_align(new_size, align) {
        Some(layout) => layout,
        None => return core::ptr::null_mut(),
    };

    let new_ptr = allocate(new_layout);
    if new_ptr.is_null() {
        return core::ptr::null_mut();
    }

    let copy_size = core::cmp::min(old_size, new_size);
    copy_nonoverlapping(ptr, new_ptr, copy_size);
    __rust_dealloc(ptr, old_size, align);
    new_ptr
}

#[no_mangle]
pub unsafe extern "C" fn __rustc__rust_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    __rust_realloc(ptr, old_size, align, new_size)
}

#[export_name = "__rustc::__rust_realloc"]
pub unsafe extern "C" fn __rustc_colon__rust_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    __rust_realloc(ptr, old_size, align, new_size)
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc14___rust_realloc"]
pub unsafe extern "C" fn __rustc_mangled_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    __rust_realloc(ptr, old_size, align, new_size)
}

#[no_mangle]
pub extern "C" fn __rust_alloc_error_handler(size: usize, align: usize) -> ! {
    let layout = unsafe { layout_from_size_align(size, align) }
        .unwrap_or_else(|| Layout::from_size_align(align, align).unwrap());
    handle_alloc_error(layout)
}

#[no_mangle]
pub extern "C" fn __rust_alloc_error_handler_should_panic() -> bool {
    true
}

#[no_mangle]
pub extern "C" fn __rustc__rust_alloc_error_handler(size: usize, align: usize) -> ! {
    __rust_alloc_error_handler(size, align)
}

#[no_mangle]
pub extern "C" fn __rustc__rust_alloc_error_handler_should_panic() -> bool {
    __rust_alloc_error_handler_should_panic()
}

#[export_name = "__rustc::__rust_alloc_error_handler"]
pub extern "C" fn __rustc_colon__rust_alloc_error_handler(size: usize, align: usize) -> ! {
    __rust_alloc_error_handler(size, align)
}

#[export_name = "__rustc::__rust_alloc_error_handler_should_panic"]
pub extern "C" fn __rustc_colon__rust_alloc_error_handler_should_panic() -> bool {
    __rust_alloc_error_handler_should_panic()
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc26___rust_alloc_error_handler"]
pub extern "C" fn __rustc_mangled_alloc_error_handler(size: usize, align: usize) -> ! {
    __rust_alloc_error_handler(size, align)
}

#[export_name = "_RNvCs691rhTbG0Ee_7___rustc39___rust_alloc_error_handler_should_panic"]
pub extern "C" fn __rustc_mangled_alloc_error_handler_should_panic() -> bool {
    __rust_alloc_error_handler_should_panic()
}

core::arch::global_asm!(
    ".globl __rust_no_alloc_shim_is_unstable.0\n__rust_no_alloc_shim_is_unstable.0 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.1\n__rust_no_alloc_shim_is_unstable.1 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.2\n__rust_no_alloc_shim_is_unstable.2 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.3\n__rust_no_alloc_shim_is_unstable.3 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.4\n__rust_no_alloc_shim_is_unstable.4 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.5\n__rust_no_alloc_shim_is_unstable.5 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.6\n__rust_no_alloc_shim_is_unstable.6 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.7\n__rust_no_alloc_shim_is_unstable.7 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.8\n__rust_no_alloc_shim_is_unstable.8 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.9\n__rust_no_alloc_shim_is_unstable.9 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.10\n__rust_no_alloc_shim_is_unstable.10 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.11\n__rust_no_alloc_shim_is_unstable.11 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.12\n__rust_no_alloc_shim_is_unstable.12 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.13\n__rust_no_alloc_shim_is_unstable.13 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.14\n__rust_no_alloc_shim_is_unstable.14 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.15\n__rust_no_alloc_shim_is_unstable.15 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.16\n__rust_no_alloc_shim_is_unstable.16 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.17\n__rust_no_alloc_shim_is_unstable.17 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.18\n__rust_no_alloc_shim_is_unstable.18 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.19\n__rust_no_alloc_shim_is_unstable.19 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.20\n__rust_no_alloc_shim_is_unstable.20 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.21\n__rust_no_alloc_shim_is_unstable.21 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.22\n__rust_no_alloc_shim_is_unstable.22 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.23\n__rust_no_alloc_shim_is_unstable.23 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.24\n__rust_no_alloc_shim_is_unstable.24 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.25\n__rust_no_alloc_shim_is_unstable.25 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.26\n__rust_no_alloc_shim_is_unstable.26 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.27\n__rust_no_alloc_shim_is_unstable.27 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.28\n__rust_no_alloc_shim_is_unstable.28 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.29\n__rust_no_alloc_shim_is_unstable.29 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.30\n__rust_no_alloc_shim_is_unstable.30 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.31\n__rust_no_alloc_shim_is_unstable.31 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.32\n__rust_no_alloc_shim_is_unstable.32 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.33\n__rust_no_alloc_shim_is_unstable.33 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.34\n__rust_no_alloc_shim_is_unstable.34 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.35\n__rust_no_alloc_shim_is_unstable.35 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.36\n__rust_no_alloc_shim_is_unstable.36 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.37\n__rust_no_alloc_shim_is_unstable.37 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.38\n__rust_no_alloc_shim_is_unstable.38 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.39\n__rust_no_alloc_shim_is_unstable.39 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.40\n__rust_no_alloc_shim_is_unstable.40 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.41\n__rust_no_alloc_shim_is_unstable.41 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.42\n__rust_no_alloc_shim_is_unstable.42 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.43\n__rust_no_alloc_shim_is_unstable.43 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.44\n__rust_no_alloc_shim_is_unstable.44 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.45\n__rust_no_alloc_shim_is_unstable.45 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.46\n__rust_no_alloc_shim_is_unstable.46 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.47\n__rust_no_alloc_shim_is_unstable.47 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.48\n__rust_no_alloc_shim_is_unstable.48 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.49\n__rust_no_alloc_shim_is_unstable.49 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.50\n__rust_no_alloc_shim_is_unstable.50 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.51\n__rust_no_alloc_shim_is_unstable.51 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.52\n__rust_no_alloc_shim_is_unstable.52 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.53\n__rust_no_alloc_shim_is_unstable.53 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.54\n__rust_no_alloc_shim_is_unstable.54 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.55\n__rust_no_alloc_shim_is_unstable.55 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.56\n__rust_no_alloc_shim_is_unstable.56 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.57\n__rust_no_alloc_shim_is_unstable.57 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.58\n__rust_no_alloc_shim_is_unstable.58 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.59\n__rust_no_alloc_shim_is_unstable.59 = __rust_no_alloc_shim_is_unstable",
    ".globl __rust_no_alloc_shim_is_unstable.60\n__rust_no_alloc_shim_is_unstable.60 = __rust_no_alloc_shim_is_unstable"
);

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
