#![allow(dead_code)]

use crate::klog;
use crate::mem::heap;
use crate::sync::spinlock::SpinLock;

use core::alloc::Layout;
use core::{ptr, slice};

pub mod console;
pub mod keyboard;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DriverKind {
    Block,
    Char,
}

#[derive(Debug)]
pub enum DriverError {
    RegistryFull,
    InitFailed,
    Unsupported,
    IoError,
}

pub trait Driver: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> DriverKind;
    fn init(&self) -> Result<(), DriverError>;
    fn shutdown(&self) {}
}

pub trait BlockDevice: Driver {
    fn block_size(&self) -> usize;
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), DriverError>;
    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), DriverError>;

    fn flush(&self) -> Result<(), DriverError> {
        Ok(())
    }
}

pub trait CharDevice: Driver {
    fn read(&self, buf: &mut [u8]) -> Result<usize, DriverError>;
    fn write(&self, buf: &[u8]) -> Result<usize, DriverError>;
}

#[derive(Copy, Clone)]
enum DriverSlot {
    Empty,
    Block(&'static dyn BlockDevice),
    Char(&'static dyn CharDevice),
}

impl DriverSlot {
    const fn empty() -> Self {
        DriverSlot::Empty
    }

    fn kind(&self) -> Option<DriverKind> {
        match self {
            DriverSlot::Empty => None,
            DriverSlot::Block(_) => Some(DriverKind::Block),
            DriverSlot::Char(_) => Some(DriverKind::Char),
        }
    }

    fn name(&self) -> Option<&'static str> {
        match self {
            DriverSlot::Empty => None,
            DriverSlot::Block(dev) => Some(dev.name()),
            DriverSlot::Char(dev) => Some(dev.name()),
        }
    }

    fn as_char(&self) -> Option<&'static dyn CharDevice> {
        match self {
            DriverSlot::Char(dev) => Some(*dev),
            _ => None,
        }
    }

    fn as_block(&self) -> Option<&'static dyn BlockDevice> {
        match self {
            DriverSlot::Block(dev) => Some(*dev),
            _ => None,
        }
    }
}

struct DriverRegistry {
    slots: *mut DriverSlot,
    len: usize,
    capacity: usize,
}

unsafe impl Send for DriverRegistry {}

impl DriverRegistry {
    const fn new() -> Self {
        Self {
            slots: ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    fn register_block(&mut self, device: &'static dyn BlockDevice) -> Result<(), DriverError> {
        self.insert(DriverSlot::Block(device))
    }

    fn register_char(&mut self, device: &'static dyn CharDevice) -> Result<(), DriverError> {
        self.insert(DriverSlot::Char(device))
    }

    fn insert(&mut self, slot: DriverSlot) -> Result<(), DriverError> {
        self.ensure_capacity(1)?;
        unsafe {
            self.slots.add(self.len).write(slot);
        }
        self.len += 1;
        Ok(())
    }

    fn ensure_capacity(&mut self, additional: usize) -> Result<(), DriverError> {
        let required = self.len.checked_add(additional).ok_or(DriverError::RegistryFull)?;
        if required <= self.capacity {
            return Ok(());
        }

        debug_assert!(self.slots.is_null() == (self.capacity == 0));

        let mut new_capacity = if self.capacity == 0 { 4 } else { self.capacity };
        while new_capacity < required {
            new_capacity = new_capacity.saturating_mul(2);
        }

        let layout = Layout::array::<DriverSlot>(new_capacity).map_err(|_| DriverError::RegistryFull)?;
        let new_ptr = unsafe { heap::allocate(layout) } as *mut DriverSlot;
        if new_ptr.is_null() {
            return Err(DriverError::RegistryFull);
        }

        unsafe {
            if self.len > 0 {
                ptr::copy_nonoverlapping(self.slots, new_ptr, self.len);
            }

            for index in self.len..new_capacity {
                new_ptr.add(index).write(DriverSlot::empty());
            }
        }

        if self.capacity != 0 {
            let old_layout = Layout::array::<DriverSlot>(self.capacity).map_err(|_| DriverError::RegistryFull)?;
            unsafe {
                heap::deallocate(self.slots as *mut u8, old_layout);
            }
        }

        self.slots = new_ptr;
        self.capacity = new_capacity;
        Ok(())
    }

    fn iter(&self) -> impl Iterator<Item = &DriverSlot> {
        let slice: &[DriverSlot] = if self.len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.slots, self.len) }
        };
        slice.iter()
    }
}

impl Drop for DriverRegistry {
    fn drop(&mut self) {
        if self.capacity == 0 || self.slots.is_null() {
            return;
        }

        if let Ok(layout) = Layout::array::<DriverSlot>(self.capacity) {
            unsafe {
                heap::deallocate(self.slots as *mut u8, layout);
            }
        }

        self.slots = ptr::null_mut();
        self.len = 0;
        self.capacity = 0;
    }
}

static REGISTRY: SpinLock<DriverRegistry> = SpinLock::new(DriverRegistry::new());

mod builtin;

pub fn init() {
    klog!("[driver] registry ready\n");
}

pub fn register_block(device: &'static dyn BlockDevice) -> Result<(), DriverError> {
    device.init().map_err(|_| DriverError::InitFailed)?;
    let mut registry = REGISTRY.lock();
    registry.register_block(device)?;
    klog!("[driver] registered block device '{}'\n", device.name());
    Ok(())
}

pub fn register_char(device: &'static dyn CharDevice) -> Result<(), DriverError> {
    device.init().map_err(|_| DriverError::InitFailed)?;
    let mut registry = REGISTRY.lock();
    registry.register_char(device)?;
    klog!("[driver] registered char device '{}'\n", device.name());
    Ok(())
}

pub fn list_drivers() {
    let registry = REGISTRY.lock();
    for slot in registry.iter() {
        if let (Some(name), Some(kind)) = (slot.name(), slot.kind()) {
            klog!("[driver] {} ({:?})\n", name, kind);
        }
    }
}

pub fn register_builtin() {
    builtin::register();
}

pub fn for_each_char_device<F>(mut f: F)
where
    F: FnMut(&'static dyn CharDevice),
{
    let registry = REGISTRY.lock();
    for slot in registry.iter() {
        if let Some(dev) = slot.as_char() {
            f(dev);
        }
    }
}

pub fn for_each_block_device<F>(mut f: F)
where
    F: FnMut(&'static dyn BlockDevice),
{
    let registry = REGISTRY.lock();
    for slot in registry.iter() {
        if let Some(dev) = slot.as_block() {
            f(dev);
        }
    }
}

pub fn block_device_by_name(name: &str) -> Option<&'static dyn BlockDevice> {
    let registry = REGISTRY.lock();
    for slot in registry.iter() {
        if let Some(dev) = slot.as_block() {
            if dev.name() == name {
                return Some(dev);
            }
        }
    }
    None
}
