use crate::mem::phys;

use super::mmu;

pub const PAGE_SIZE: usize = 4096;
const PAGE_TABLE_ENTRIES: usize = 512;
const ENTRY_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

pub const FLAG_PRESENT: u64 = 1 << 0;
pub const FLAG_WRITABLE: u64 = 1 << 1;
pub const FLAG_USER: u64 = 1 << 2;
pub const FLAG_WRITE_THROUGH: u64 = 1 << 3;
pub const FLAG_CACHE_DISABLE: u64 = 1 << 4;
pub const FLAG_HUGE: u64 = 1 << 7;
pub const FLAG_NO_EXECUTE: u64 = 1 << 63;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MapError {
    OutOfMemory,
    AlreadyMapped,
}

type PageTable = [u64; PAGE_TABLE_ENTRIES];

fn table_from_phys(phys: u64) -> &'static mut PageTable {
    let virt = mmu::phys_to_virt(phys);
    unsafe { &mut *(virt as *mut PageTable) }
}

fn allocate_table() -> Result<(u64, &'static mut PageTable), MapError> {
    let frame = phys::allocate_frame().ok_or(MapError::OutOfMemory)?;
    let phys = frame.start();
    let table = table_from_phys(phys);
    for entry in table.iter_mut() {
        *entry = 0;
    }
    Ok((phys, table))
}

pub fn clone_kernel_pml4() -> Result<u64, MapError> {
    let kernel_cr3 = unsafe { mmu::read_cr3() };
    let kernel = table_from_phys(kernel_cr3);

    let (new_phys, new_table) = allocate_table()?;

    // Copy higher-half entries so the kernel remains mapped
    new_table[256..].copy_from_slice(&kernel[256..]);
    Ok(new_phys)
}

fn ensure_table(entry: &mut u64, user: bool) -> Result<&'static mut PageTable, MapError> {
    if *entry & FLAG_PRESENT == 0 {
        let (phys, table) = allocate_table()?;
        let mut flags = FLAG_PRESENT | FLAG_WRITABLE;
        if user {
            flags |= FLAG_USER;
        }
        *entry = phys | flags;
        Ok(table)
    } else {
        let phys = *entry & ENTRY_ADDR_MASK;
        Ok(table_from_phys(phys))
    }
}

#[inline]
fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1FF) as usize
}

#[inline]
fn pdpt_index(addr: u64) -> usize {
    ((addr >> 30) & 0x1FF) as usize
}

#[inline]
fn pd_index(addr: u64) -> usize {
    ((addr >> 21) & 0x1FF) as usize
}

#[inline]
fn pt_index(addr: u64) -> usize {
    ((addr >> 12) & 0x1FF) as usize
}

pub fn map_page(
    pml4_phys: u64,
    virt_addr: u64,
    frame_phys: u64,
    flags: u64,
) -> Result<(), MapError> {
    if virt_addr & 0xFFF != 0 || frame_phys & 0xFFF != 0 {
        return Err(MapError::AlreadyMapped);
    }

    let user = flags & FLAG_USER != 0;

    let pml4 = table_from_phys(pml4_phys);
    let pml4e = &mut pml4[pml4_index(virt_addr)];
    let pdpt = ensure_table(pml4e, user)?;

    let pdpte = &mut pdpt[pdpt_index(virt_addr)];
    let pd = ensure_table(pdpte, user)?;
    if *pdpte & FLAG_HUGE != 0 {
        return Err(MapError::AlreadyMapped);
    }

    let pde = &mut pd[pd_index(virt_addr)];
    let pt = ensure_table(pde, user)?;
    if *pde & FLAG_HUGE != 0 {
        return Err(MapError::AlreadyMapped);
    }

    let pte = &mut pt[pt_index(virt_addr)];
    if *pte & FLAG_PRESENT != 0 {
        return Err(MapError::AlreadyMapped);
    }

    *pte = frame_phys | (flags | FLAG_PRESENT);
    Ok(())
}

pub fn unmap_page(pml4_phys: u64, virt_addr: u64) {
    if virt_addr & 0xFFF != 0 {
        return;
    }

    let pml4 = table_from_phys(pml4_phys);
    let pml4e = pml4[pml4_index(virt_addr)];
    if pml4e & FLAG_PRESENT == 0 {
        return;
    }
    let pdpt = table_from_phys(pml4e & ENTRY_ADDR_MASK);

    let pdpte = pdpt[pdpt_index(virt_addr)];
    if pdpte & FLAG_PRESENT == 0 || pdpte & FLAG_HUGE != 0 {
        return;
    }
    let pd = table_from_phys(pdpte & ENTRY_ADDR_MASK);

    let pde = pd[pd_index(virt_addr)];
    if pde & FLAG_PRESENT == 0 || pde & FLAG_HUGE != 0 {
        return;
    }
    let pt = table_from_phys(pde & ENTRY_ADDR_MASK);

    let pte = &mut pt[pt_index(virt_addr)];
    *pte = 0;
}

pub fn translate(pml4_phys: u64, virt_addr: u64) -> Option<u64> {
    let pml4 = table_from_phys(pml4_phys);
    let pml4e = pml4[pml4_index(virt_addr)];
    if pml4e & FLAG_PRESENT == 0 {
        return None;
    }

    let pdpt = table_from_phys(pml4e & ENTRY_ADDR_MASK);
    let pdpte = pdpt[pdpt_index(virt_addr)];
    if pdpte & FLAG_PRESENT == 0 {
        return None;
    }

    if pdpte & FLAG_HUGE != 0 {
        let base = pdpte & ENTRY_ADDR_MASK;
        let offset = virt_addr & ((1 << 30) - 1);
        return Some(base + offset);
    }

    let pd = table_from_phys(pdpte & ENTRY_ADDR_MASK);
    let pde = pd[pd_index(virt_addr)];
    if pde & FLAG_PRESENT == 0 {
        return None;
    }

    if pde & FLAG_HUGE != 0 {
        let base = pde & ENTRY_ADDR_MASK;
        let offset = virt_addr & ((1 << 21) - 1);
        return Some(base + offset);
    }

    let pt = table_from_phys(pde & ENTRY_ADDR_MASK);
    let pte = pt[pt_index(virt_addr)];
    if pte & FLAG_PRESENT == 0 {
        return None;
    }
    let base = pte & ENTRY_ADDR_MASK;
    let offset = virt_addr & 0xFFF;
    Some(base + offset)
}
