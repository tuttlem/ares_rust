use crate::arch::x86_64::io::inb;
use crate::arch::x86_64::kernel::interrupts;
use crate::arch::x86_64::kernel::interrupts::InterruptFrame;
use crate::klog;
use crate::process::{self, WaitChannel};
use crate::sync::spinlock::SpinLock;

const DATA_PORT: u16 = 0x60;
const BUFFER_SIZE: usize = 256;

static STATE: SpinLock<KeyboardState> = SpinLock::new(KeyboardState::new());
static INIT: SpinLock<bool> = SpinLock::new(false);

struct KeyboardState {
    buffer: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
    shift: bool,
    caps_lock: bool,
}

impl KeyboardState {
    const fn new() -> Self {
        Self {
            buffer: [0; BUFFER_SIZE],
            head: 0,
            tail: 0,
            shift: false,
            caps_lock: false,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.is_full() {
            // drop oldest value to make room
            let dropped = self.buffer[self.head];
            klog!(
                "[keyboard] buffer full, dropping oldest byte 0x{:02X} head={} tail={}\n",
                dropped,
                self.head,
                self.tail
            );
            self.head = (self.head + 1) % BUFFER_SIZE;
        }
        self.buffer[self.tail] = byte;
        self.tail = (self.tail + 1) % BUFFER_SIZE;
    }

    fn pop(&mut self) -> Option<u8> {
        if self.is_empty() {
            return None;
        }
        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % BUFFER_SIZE;
        Some(byte)
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    fn is_full(&self) -> bool {
        (self.tail + 1) % BUFFER_SIZE == self.head
    }
}

pub fn init() {
    let mut flag = INIT.lock();
    if *flag {
        return;
    }

    interrupts::register_handler(interrupts::vectors::KEYBOARD, keyboard_handler);
    interrupts::enable_vector(interrupts::vectors::KEYBOARD);
    *flag = true;
    klog!("[keyboard] PS/2 keyboard initialized\n");
}

pub fn read(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }

    let mut state = STATE.lock();
    if let Some(byte) = state.pop() {
        buf[0] = byte;
        1
    } else {
        0
    }
}

fn keyboard_handler(_frame: &mut InterruptFrame) {
    let scancode = unsafe { inb(DATA_PORT) };

    let mut state = STATE.lock();
    let mut pushed = false;

    if scancode & 0x80 != 0 {
        handle_key_release(&mut state, scancode & 0x7F);
    } else {
        if let Some(byte) = translate_scancode(&mut state, scancode) {
            state.push(byte);
            pushed = true;
        }
    }

    drop(state);

    if pushed {
        process::wake_channel(WaitChannel::KeyboardInput);
    }
}

fn handle_key_release(state: &mut KeyboardState, scancode: u8) {
    match scancode {
        0x2A | 0x36 => state.shift = false,
        _ => {}
    }
}

fn translate_scancode(state: &mut KeyboardState, scancode: u8) -> Option<u8> {
    match scancode {
        0x2A | 0x36 => {
            state.shift = true;
            None
        }
        0x3A => {
            state.caps_lock = !state.caps_lock;
            None
        }
        0x1C => Some(b'\n'),
        0x0E => Some(0x08), // backspace
        0x0F => Some(b'\t'),
        0x39 => Some(b' '),
        0x10..=0x19 | 0x1E..=0x26 | 0x2C..=0x32 => map_letter(scancode, state.shift, state.caps_lock),
        _ => map_symbol(scancode, state.shift),
    }
}

fn map_letter(scancode: u8, shift: bool, caps: bool) -> Option<u8> {
    let letter = match scancode {
        0x10 => b'q',
        0x11 => b'w',
        0x12 => b'e',
        0x13 => b'r',
        0x14 => b't',
        0x15 => b'y',
        0x16 => b'u',
        0x17 => b'i',
        0x18 => b'o',
        0x19 => b'p',
        0x1E => b'a',
        0x1F => b's',
        0x20 => b'd',
        0x21 => b'f',
        0x22 => b'g',
        0x23 => b'h',
        0x24 => b'j',
        0x25 => b'k',
        0x26 => b'l',
        0x2C => b'z',
        0x2D => b'x',
        0x2E => b'c',
        0x2F => b'v',
        0x30 => b'b',
        0x31 => b'n',
        0x32 => b'm',
        _ => return None,
    };

    let use_shift = shift ^ caps;
    let ch = if use_shift {
        letter.to_ascii_uppercase()
    } else {
        letter
    };

    Some(ch)
}

fn map_symbol(scancode: u8, shift: bool) -> Option<u8> {
    let byte = match scancode {
        0x02 => if shift { b'!' } else { b'1' },
        0x03 => if shift { b'@' } else { b'2' },
        0x04 => if shift { b'#' } else { b'3' },
        0x05 => if shift { b'$' } else { b'4' },
        0x06 => if shift { b'%' } else { b'5' },
        0x07 => if shift { b'^' } else { b'6' },
        0x08 => if shift { b'&' } else { b'7' },
        0x09 => if shift { b'*' } else { b'8' },
        0x0A => if shift { b'(' } else { b'9' },
        0x0B => if shift { b')' } else { b'0' },
        0x0C => if shift { b'_' } else { b'-' },
        0x0D => if shift { b'+' } else { b'=' },
        0x1A => if shift { b'{' } else { b'[' },
        0x1B => if shift { b'}' } else { b']' },
        0x27 => if shift { b':' } else { b';' },
        0x28 => if shift { b'"' } else { b'\'' },
        0x29 => if shift { b'~' } else { b'`' },
        0x2B => if shift { b'|' } else { b'\\' },
        0x33 => if shift { b'<' } else { b',' },
        0x34 => if shift { b'>' } else { b'.' },
        0x35 => if shift { b'?' } else { b'/' },
        _ => 0,
    };

    if byte == 0 {
        None
    } else {
        Some(byte)
    }
}
