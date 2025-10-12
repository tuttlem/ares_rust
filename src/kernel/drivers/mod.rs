#![allow(dead_code)]

use crate::klog;
use crate::mem::heap;
use crate::sync::spinlock::SpinLock;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DriverKind {
    Block,
    Char,
}

#[derive(Debug)]
pub enum DriverError {
    AllocationFailed,
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

    fn as_block(&self) -> Option<&'static dyn BlockDevice> {
        match self {
            DriverSlot::Block(dev) => Some(*dev),
            _ => None,
        }
    }

    fn as_char(&self) -> Option<&'static dyn CharDevice> {
        match self {
            DriverSlot::Char(dev) => Some(*dev),
            _ => None,
        }
    }
}

struct DriverNode {
    slot: DriverSlot,
    next: Option<&'static DriverNode>,
}

struct DriverRegistry {
    head: Option<&'static DriverNode>,
    count: usize,
}

impl DriverRegistry {
    const fn new() -> Self {
        Self {
            head: None,
            count: 0,
        }
    }

    fn register_block(&mut self, device: &'static dyn BlockDevice) -> Result<(), DriverError> {
        self.insert(DriverSlot::Block(device))
    }

    fn register_char(&mut self, device: &'static dyn CharDevice) -> Result<(), DriverError> {
        self.insert(DriverSlot::Char(device))
    }

    fn insert(&mut self, slot: DriverSlot) -> Result<(), DriverError> {
        let layout = core::alloc::Layout::new::<DriverNode>();
        let ptr = unsafe { heap::allocate(layout) as *mut DriverNode };
        if ptr.is_null() {
            return Err(DriverError::AllocationFailed);
        }

        unsafe {
            ptr.write(DriverNode {
                slot,
                next: self.head,
            });
            self.head = Some(&*ptr);
            self.count += 1;
        }

        Ok(())
    }

    fn iter(&self) -> DriverIter {
        DriverIter { current: self.head }
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

pub fn self_test() {
    for_each_char_device(|dev| {
        let mut buffer = [0u8; 16];
        if let Ok(bytes) = dev.read(&mut buffer) {
            klog!(
                "[driver] test read {} bytes from '{}': {:02X?}\n",
                bytes,
                dev.name(),
                &buffer[..bytes.min(buffer.len())]
            );
        }

        let payload = b"driver-test";
        if let Ok(bytes) = dev.write(payload) {
            klog!(
                "[driver] test wrote {} bytes to '{}'\n",
                bytes,
                dev.name()
            );
        }
    });
}

struct DriverIter {
    current: Option<&'static DriverNode>,
}

impl Iterator for DriverIter {
    type Item = &'static DriverSlot;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(node) = self.current {
            self.current = node.next;
            Some(&node.slot)
        } else {
            None
        }
    }
}
