#![allow(dead_code)]

pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidAlignment,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Page,
    HugePage,
}

pub fn align_up(addr: u64, align: u64) -> Result<u64, Error> {
    if align.count_ones() != 1 {
        return Err(Error::InvalidAlignment);
    }
    let mask = align - 1;
    Ok((addr + mask) & !mask)
}

pub fn align_down(addr: u64, align: u64) -> Result<u64, Error> {
    if align.count_ones() != 1 {
        return Err(Error::InvalidAlignment);
    }
    let mask = align - 1;
    Ok(addr & !mask)
}

pub fn pages_required(length: u64, page_size: u64) -> u64 {
    if length == 0 {
        return 0;
    }
    ((length - 1) / page_size) + 1
}

pub fn is_aligned(addr: u64, align: u64) -> bool {
    if align.count_ones() != 1 {
        return false;
    }
    addr & (align - 1) == 0
}
