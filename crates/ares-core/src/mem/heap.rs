use core::alloc::Layout;

#[cfg(feature = "std")]
pub unsafe fn allocate(layout: Layout) -> *mut u8 {
    std::alloc::alloc(layout)
}

#[cfg(feature = "std")]
pub unsafe fn deallocate(ptr: *mut u8, layout: Layout) {
    if !ptr.is_null() {
        std::alloc::dealloc(ptr, layout);
    }
}

#[cfg(not(feature = "std"))]
pub unsafe fn allocate(_layout: Layout) -> *mut u8 {
    core::ptr::null_mut()
}

#[cfg(not(feature = "std"))]
pub unsafe fn deallocate(_ptr: *mut u8, _layout: Layout) {}
