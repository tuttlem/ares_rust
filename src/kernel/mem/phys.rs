#![allow(dead_code)]

use crate::klog;
use crate::sync::spinlock::SpinLock;

const MAX_REGIONS: usize = 128;
const PAGE_SIZE: u64 = 4096;

#[derive(Copy, Clone, Debug)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
}

impl MemoryRegion {
    const fn empty() -> Self {
        Self { base: 0, length: 0 }
    }

    pub fn end(&self) -> u64 {
        self.base + self.length
    }

    pub fn page_count(&self) -> u64 {
        (self.length + PAGE_SIZE - 1) / PAGE_SIZE
    }
}

struct MemoryMap {
    regions: [MemoryRegion; MAX_REGIONS],
    count: usize,
}

impl MemoryMap {
    const fn new() -> Self {
        Self {
            regions: [MemoryRegion::empty(); MAX_REGIONS],
            count: 0,
        }
    }

    fn clear(&mut self) {
        self.count = 0;
    }

    fn add_region(&mut self, region: MemoryRegion) {
        if self.count < MAX_REGIONS {
            self.regions[self.count] = region;
            self.count += 1;
        } else {
            klog!("[phys] region table full, dropping entry base=0x{:016X} len=0x{:016X}\n", region.base, region.length);
        }
    }

    fn iter(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions[..self.count].iter()
    }
}

static PHYS_MEMORY_MAP: SpinLock<MemoryMap> = SpinLock::new(MemoryMap::new());

#[repr(C)]
struct TagHeader {
    tag_type: u32,
    size: u32,
}

#[repr(C)]
struct MemoryMapTagHeader {
    header: TagHeader,
    entry_size: u32,
    entry_version: u32,
}

#[repr(C)]
struct MemoryMapEntry {
    base_addr: u64,
    length: u64,
    entry_type: u32,
    _reserved: u32,
}

const TAG_TYPE_END: u32 = 0;
const TAG_TYPE_MMAP: u32 = 6;
const MEMORY_TYPE_AVAILABLE: u32 = 1;

#[derive(Copy, Clone)]
pub struct MemorySummary {
    pub region_count: usize,
    pub total_bytes: u64,
}

pub fn init(multiboot_info_addr: usize) {
    unsafe {
        parse(multiboot_info_addr);
    }

    let summary = summary();
    klog!(
        "[phys] {} usable region(s), total {:>6} KiB\n",
        summary.region_count,
        summary.total_bytes / 1024
    );

    for_each_region(|region| {
        klog!(
            "[phys] usable: base=0x{:016X} len=0x{:016X} pages={}\n",
            region.base,
            region.length,
            region.page_count()
        );
    });
}

pub fn for_each_region<F>(mut f: F)
where
    F: FnMut(&MemoryRegion),
{
    let map = PHYS_MEMORY_MAP.lock();
    for region in map.iter() {
        f(region);
    }
}

pub fn summary() -> MemorySummary {
    let map = PHYS_MEMORY_MAP.lock();
    let mut total = 0u64;
    for region in map.iter() {
        total = total.saturating_add(region.length);
    }
    MemorySummary {
        region_count: map.count,
        total_bytes: total,
    }
}

unsafe fn parse(multiboot_info_addr: usize) {
    let total_size = *(multiboot_info_addr as *const u32) as usize;
    let mut current = multiboot_info_addr + core::mem::size_of::<u32>() * 2;
    let end = multiboot_info_addr + total_size;

    let mut map = PHYS_MEMORY_MAP.lock();
    map.clear();

    while current < end {
        let header = &*(current as *const TagHeader);
        if header.tag_type == TAG_TYPE_END {
            break;
        }

        if header.tag_type == TAG_TYPE_MMAP {
            parse_memory_map_tag(current as *const MemoryMapTagHeader, &mut map);
        }

        current = align_up(current + header.size as usize, 8);
    }
}

unsafe fn parse_memory_map_tag(ptr: *const MemoryMapTagHeader, map: &mut MemoryMap) {
    let tag = &*ptr;
    let entry_size = tag.entry_size as usize;
    let entries_start = (ptr as usize) + core::mem::size_of::<MemoryMapTagHeader>();
    let entries_end = (ptr as usize) + tag.header.size as usize;

    let mut current = entries_start;
    while current + entry_size <= entries_end {
        let entry = &*(current as *const MemoryMapEntry);
        if entry.entry_type == MEMORY_TYPE_AVAILABLE && entry.length > 0 {
            map.add_region(MemoryRegion {
                base: entry.base_addr,
                length: entry.length,
            });
        }
        current += entry_size;
    }
}

fn align_up(value: usize, align: usize) -> usize {
    let mask = align - 1;
    (value + mask) & !mask
}
