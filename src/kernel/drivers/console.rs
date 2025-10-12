use crate::drivers::{CharDevice, Driver, DriverError, DriverKind};
use crate::sync::spinlock::SpinLock;

#[cfg(target_arch = "x86_64")]
#[path = "../../arch/x86_64/drivers/console.rs"]
mod arch;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("Console driver is only implemented for x86_64");

pub struct Console;

struct ConsoleState {
    row: usize,
    col: usize,
    attr: u8,
}

static CONSOLE: Console = Console;
static STATE: SpinLock<ConsoleState> = SpinLock::new(ConsoleState {
    row: 0,
    col: 0,
    attr: arch::DEFAULT_ATTR,
});

impl Console {
    pub fn instance() -> &'static Console {
        &CONSOLE
    }
}

impl Driver for Console {
    fn name(&self) -> &'static str {
        "console"
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Char
    }

    fn init(&self) -> Result<(), DriverError> {
        let mut state = STATE.lock();
        arch::clear_screen();
        state.row = 0;
        state.col = 0;
        Ok(())
    }
}

impl CharDevice for Console {
    fn read(&self, _buf: &mut [u8]) -> Result<usize, DriverError> {
        Ok(0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, DriverError> {
        let mut state = STATE.lock();
        for &byte in buf {
            match byte {
                b'\n' => new_line(&mut state),
                b'\r' => state.col = 0,
                b'\t' => {
                    let next_tab = (state.col / 8 + 1) * 8;
                    if next_tab >= arch::WIDTH {
                        new_line(&mut state);
                    } else {
                        state.col = next_tab;
                    }
                }
                byte => put_char(&mut state, byte),
            }
        }
        Ok(buf.len())
    }
}

fn put_char(state: &mut ConsoleState, byte: u8) {
    if state.col >= arch::WIDTH {
        new_line(state);
    }

    arch::write_at(state.row, state.col, byte, state.attr);
    state.col += 1;
}

fn new_line(state: &mut ConsoleState) {
    state.col = 0;
    state.row += 1;
    if state.row >= arch::HEIGHT {
        arch::scroll_up();
        state.row = arch::HEIGHT - 1;
    }
}

pub fn driver() -> &'static dyn CharDevice {
    Console::instance()
}

pub fn write_bytes(buf: &[u8]) -> Result<usize, DriverError> {
    driver().write(buf)
}

pub fn clear() {
    let mut state = STATE.lock();
    arch::clear_screen();
    state.row = 0;
    state.col = 0;
}
