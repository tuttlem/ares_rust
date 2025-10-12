#[cfg(target_arch = "x86_64")]
#[path = "../../arch/x86_64/kernel/serial.rs"]
mod serial;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("klog serial backend not implemented for this architecture");

use core::fmt::{self, Write};

pub fn init() {
    serial::init();
}

pub fn write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        serial::write_byte(byte);
    }
}

pub fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}

pub fn writeln(s: &str) {
    write_str(s);
    write_bytes(b"\n");
}

pub fn write_fmt(args: fmt::Arguments) {
    let _ = SerialWriter.write_fmt(args);
}

#[macro_export]
macro_rules! klog {
    ($($arg:tt)*) => {
        $crate::klog::write_fmt(format_args!($($arg)*))
    };
}

struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_bytes(s.as_bytes());
        Ok(())
    }
}
