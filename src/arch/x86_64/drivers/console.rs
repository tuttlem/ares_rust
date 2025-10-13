use crate::arch::x86_64::io::{inb, outb};
use crate::sync::spinlock::SpinLock;

const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const CRTC_ADDRESS: u16 = 0x3D4;
const CRTC_DATA: u16 = 0x3D5;
pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
pub const DEFAULT_ATTR: u8 = 0x0F; // white on black

struct CursorState {
    saved: u16,
    block: u16,
    position: usize,
    active: bool,
}

impl CursorState {
    const fn new() -> Self {
        Self {
            saved: 0,
            block: 0,
            position: 0,
            active: false,
        }
    }
}

static CURSOR: SpinLock<CursorState> = SpinLock::new(CursorState::new());

#[inline(always)]
pub fn write_at(row: usize, col: usize, byte: u8, attr: u8) {
    let offset = row * WIDTH + col;
    let value = ((attr as u16) << 8) | byte as u16;
    unsafe {
        *VGA_BUFFER.add(offset) = value;
    }
}

pub fn clear_row(row: usize) {
    for col in 0..WIDTH {
        write_at(row, col, b' ', DEFAULT_ATTR);
    }
}

pub fn scroll_up() {
    unsafe {
        core::ptr::copy(
            VGA_BUFFER.add(WIDTH),
            VGA_BUFFER,
            WIDTH * (HEIGHT - 1),
        );
    }
    clear_row(HEIGHT - 1);
}

pub fn clear_screen() {
    for row in 0..HEIGHT {
        clear_row(row);
    }
    reset_cursor_state();
    set_cursor(0, 0);
}

pub fn init() {
    set_cursor_shape(0, 15);
    set_cursor(0, 0);
}

pub fn set_cursor(row: usize, col: usize) {
    let pos = (row * WIDTH + col).min(WIDTH * HEIGHT - 1) as u16;
    update_cursor_visual(row, col);
    unsafe {
        outb(CRTC_ADDRESS, 0x0F);
        outb(CRTC_DATA, (pos & 0xFF) as u8);
        outb(CRTC_ADDRESS, 0x0E);
        outb(CRTC_DATA, (pos >> 8) as u8);
    }
}

fn set_cursor_shape(start: u8, end: u8) {
    let start = start & 0x1F;
    let end = end & 0x1F;
    unsafe {
        outb(CRTC_ADDRESS, 0x0A);
        let mut cur_start = inb(CRTC_DATA);
        cur_start = (cur_start & 0xC0) | 0x20 | start; // disable hardware blink cursor
        outb(CRTC_DATA, cur_start);

        outb(CRTC_ADDRESS, 0x0B);
        let mut cur_end = inb(CRTC_DATA);
        cur_end = (cur_end & 0xE0) | end;
        outb(CRTC_DATA, cur_end);
    }
}

fn update_cursor_visual(row: usize, col: usize) {
    let mut cursor = CURSOR.lock();

    if cursor.active {
        unsafe {
            let current = *VGA_BUFFER.add(cursor.position);
            if current == cursor.block {
                *VGA_BUFFER.add(cursor.position) = cursor.saved;
            } else {
                cursor.saved = current;
            }
        }
        cursor.active = false;
    }

    let position = (row * WIDTH + col).min(WIDTH * HEIGHT - 1);
    unsafe {
        let cell = *VGA_BUFFER.add(position);
        cursor.saved = cell;
        cursor.position = position;
        cursor.active = true;

        let attr = ((cell >> 8) & 0xFF) as u8;
        let block = 0xDBu16 | ((attr as u16) << 8);
        cursor.block = block;
        *VGA_BUFFER.add(position) = block;
    }
}

fn reset_cursor_state() {
    let mut cursor = CURSOR.lock();
    if cursor.active {
        unsafe {
            let current = *VGA_BUFFER.add(cursor.position);
            if current == cursor.block {
                *VGA_BUFFER.add(cursor.position) = cursor.saved;
            } else {
                cursor.saved = current;
            }
        }
    }
    cursor.active = false;
    cursor.position = 0;
    cursor.saved = ((DEFAULT_ATTR as u16) << 8) | b' ' as u16;
    cursor.block = cursor.saved;
}
