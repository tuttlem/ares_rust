use crate::drivers::{CharDevice, Driver, DriverError, DriverKind};

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::drivers::keyboard as arch;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("Keyboard driver is only implemented for x86_64");

pub struct Keyboard;

static KEYBOARD: Keyboard = Keyboard;

impl Keyboard {
    pub fn instance() -> &'static Keyboard {
        &KEYBOARD
    }
}

impl Driver for Keyboard {
    fn name(&self) -> &'static str {
        "keyboard"
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Char
    }

    fn init(&self) -> Result<(), DriverError> {
        arch::init();
        Ok(())
    }
}

impl CharDevice for Keyboard {
    fn read(&self, buf: &mut [u8]) -> Result<usize, DriverError> {
        Ok(arch::read(buf))
    }

    fn write(&self, _buf: &[u8]) -> Result<usize, DriverError> {
        Err(DriverError::Unsupported)
    }
}

pub fn driver() -> &'static dyn CharDevice {
    Keyboard::instance()
}
