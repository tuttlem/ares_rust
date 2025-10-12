use crate::klog;
use super::{register_char, CharDevice, Driver, DriverError, DriverKind};

use super::console;
struct NullDevice;
struct ZeroDevice;

static NULL_DRIVER: NullDevice = NullDevice;
static ZERO_DRIVER: ZeroDevice = ZeroDevice;

impl Driver for NullDevice {
    fn name(&self) -> &'static str {
        "null"
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Char
    }

    fn init(&self) -> Result<(), DriverError> {
        Ok(())
    }
}

impl CharDevice for NullDevice {
    fn read(&self, _buf: &mut [u8]) -> Result<usize, DriverError> {
        Ok(0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, DriverError> {
        Ok(buf.len())
    }
}

impl Driver for ZeroDevice {
    fn name(&self) -> &'static str {
        "zero"
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Char
    }

    fn init(&self) -> Result<(), DriverError> {
        Ok(())
    }
}

impl CharDevice for ZeroDevice {
    fn read(&self, buf: &mut [u8]) -> Result<usize, DriverError> {
        for byte in buf.iter_mut() {
            *byte = 0;
        }
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> Result<usize, DriverError> {
        Ok(buf.len())
    }
}

pub fn register() {
    if let Err(err) = register_char(console::driver()) {
        klog!("[driver] failed to register console: {:?}\n", err);
    }
    if let Err(err) = register_char(&NULL_DRIVER) {
        klog!("[driver] failed to register null device: {:?}\n", err);
    }
    if let Err(err) = register_char(&ZERO_DRIVER) {
        klog!("[driver] failed to register zero device: {:?}\n", err);
    }
}
