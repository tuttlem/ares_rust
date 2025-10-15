# VGA Console Driver

Source: `src/arch/x86_64/drivers/console.rs` exposed via `src/kernel/drivers/console.rs`.

## Responsibilities

- Drives the VGA text buffer at `0xB8000` (80×25 grid).
- Manages a software cursor: the hardware cursor is repositioned, but a block character is painted into the buffer to provide a visible caret.
- Provides helpers used by the architecture-neutral console façade (`kernel/drivers/console.rs`) to implement `CharDevice::write`.

## Key operations

- `write_at(row, col, byte, attr)` – writes a character/attribute pair into the VGA buffer.
- `clear_row(row)` / `clear_screen()` – zero/fill lines and reset cursor state.
- `scroll_up()` – shifts the framebuffer up by one row when reaching the bottom.
- `set_cursor(row, col)` – updates both the VGA cursor registers and the software caret block.

## Synchronisation

A `SpinLock<CursorState>` protects the caret bookkeeping (`saved` cell contents, block glyph, position). Writes to the framebuffer itself do not hold the lock; only cursor updates need serialisation.

## Usage from the portable layer

`kernel/drivers/console.rs` implements the `CharDevice` trait for the console by:

1. Acquiring a mutex around current position.
2. Feeding characters into the VGA helper (handling newlines and scrolling).
3. Mirroring the output to the serial driver so logs land on both devices.

When extending the console driver (e.g., colours or escape codes) ensure the shared caret state remains consistent with the mirrored serial output to avoid cursor drift.
