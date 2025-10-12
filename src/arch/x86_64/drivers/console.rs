const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
pub const DEFAULT_ATTR: u8 = 0x0F; // white on black

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
}
