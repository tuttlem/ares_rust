#![allow(dead_code)]

use crate::arch::x86_64::kernel::mmu;
use crate::klog;
use crate::sync::spinlock::SpinLock;
use crate::mem::heap;

const MAX_REGIONS: usize = 128;
const PAGE_SIZE: u64 = 4096;
const RESERVED_END: u64 = 0x0010_0000; // keep first 1 MiB reserved (legacy floor)

pub const FRAME_SIZE: u64 = PAGE_SIZE;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    start: u64,
}

impl Frame {
    pub fn containing(addr: u64) -> Self {
        Self { start: align_down_u64(addr, PAGE_SIZE) }
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.start + FRAME_SIZE
    }

    pub fn number(&self) -> u64 {
        self.start / FRAME_SIZE
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.start as *mut u8
    }
}

#[derive(Copy, Clone, Debug)]
pub struct FrameRange {
    start: Frame,
    count: usize,
}

impl FrameRange {
    pub fn start(&self) -> Frame {
        self.start
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn iter(&self) -> FrameIter {
        FrameIter {
            next: self.start,
            remaining: self.count,
        }
    }
}

pub struct FrameIter {
    next: Frame,
    remaining: usize,
}

impl Iterator for FrameIter {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let current = self.next;
        self.next = Frame {
            start: current.start + FRAME_SIZE,
        };
        self.remaining -= 1;
        Some(current)
    }
}

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
struct FrameAllocator {
    current: u64,
    end: u64,
    region_index: usize,
}
impl FrameAllocator {
    const fn new() -> Self {
        Self {
            current: 0,
            end: 0,
            region_index: 0,
        }
    }

    fn init_from_map(&mut self, map: &MemoryMap) {
        self.region_index = 0;
        self.current = 0;
        self.end = 0;
        self.advance_to_next_region(map);
    }

    fn allocate(&mut self, map: &MemoryMap) -> Option<Frame> {
        loop {
            if self.current >= self.end {
                self.advance_to_next_region(map);
                if self.current >= self.end {
                    return None;
                }
            }

            let frame = self.current;
            self.current = self.current.saturating_add(PAGE_SIZE);

            if frame == 0 {
                continue;
            }

            return Some(Frame { start: frame });
        }
    }

    fn free(&mut self, _frame: Frame) {
        // no-op for bump allocator
    }

    fn advance_to_next_region(&mut self, map: &MemoryMap) {
        let reserve_limit = reserved_limit();
        while self.region_index < map.count {
            let region = map.regions[self.region_index];
            self.region_index += 1;
            if region.length == 0 {
                continue;
            }
            let end = region.end();
            if end <= reserve_limit {
                continue;
            }

            let start_base = region.base.max(reserve_limit);
            let start = align_up_u64(start_base, PAGE_SIZE);

            if start < end {
                self.current = start;
                self.end = end;
                return;
            }
        }

        self.current = self.end;
    }
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
static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::new());

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

pub fn allocate_frame() -> Option<Frame> {
    let map_guard = PHYS_MEMORY_MAP.lock();
    let mut allocator = FRAME_ALLOCATOR.lock();
    let frame = allocator.allocate(&map_guard);
    frame
}

pub fn allocate_frames(count: usize) -> Option<FrameRange> {
    if count == 0 {
        return None;
    }

    let map_guard = PHYS_MEMORY_MAP.lock();
    let mut allocator = FRAME_ALLOCATOR.lock();

    let first = allocator.allocate(&map_guard)?;
    let mut last = first;

    for _ in 1..count {
        match allocator.allocate(&map_guard) {
            Some(next) if next.start == last.start + FRAME_SIZE => {
                last = next;
            }
            Some(_) | None => {
                // Out-of-line allocation; we can't rewind the bump pointer,
                // so just report the contiguous sequence obtained so far.
                let span_frames = ((last.start - first.start) / FRAME_SIZE) as usize + 1;
                return Some(FrameRange {
                    start: first,
                    count: span_frames,
                });
            }
        }
    }

    Some(FrameRange {
        start: first,
        count,
    })
}

pub fn free_frame(frame: Frame) {
    let mut allocator = FRAME_ALLOCATOR.lock();
    allocator.free(frame);
}

pub fn frame_size() -> u64 {
    FRAME_SIZE
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

    FRAME_ALLOCATOR.lock().init_from_map(&map);
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

fn reserved_limit() -> u64 {
    let kernel_end = unsafe {
        extern "C" {
            static _bssEnd: u8;
            static _loadStart: u8;
        }

        let end_ptr = &_bssEnd as *const u8 as u64;
        klog!("[phys] _bssEnd virt=0x{:016X}\n", end_ptr);
        if end_ptr >= mmu::KERNEL_VMA_BASE {
            end_ptr - mmu::KERNEL_VMA_BASE
        } else {
            end_ptr
        }
    };

    let start = unsafe {
        extern "C" {
            static _loadStart: u8;
        }

        let start_ptr = &_loadStart as *const u8 as u64;
        klog!("[phys] _loadStart virt=0x{:016X}\n", start_ptr);
        if start_ptr >= mmu::KERNEL_VMA_BASE {
            start_ptr - mmu::KERNEL_VMA_BASE
        } else {
            start_ptr
        }
    };

    klog!(
        "[phys] reserved_limit kernel phys start=0x{:X} end=0x{:X}\n",
        start,
        kernel_end
    );

    let (heap_start_virt, heap_end_virt) = heap::bounds();
    let heap_end_phys = if heap_end_virt >= mmu::KERNEL_LINK_BASE as usize {
        heap_end_virt as u64 - mmu::KERNEL_LINK_BASE
    } else {
        heap_end_virt as u64
    };

    klog!(
        "[phys] heap bounds virt start=0x{:016X} end=0x{:016X} phys_end=0x{:X}\n",
        heap_start_virt,
        heap_end_virt,
        heap_end_phys
    );

    let limit = core::cmp::max(core::cmp::max(RESERVED_END, kernel_end), heap_end_phys);
    align_up_u64(limit, PAGE_SIZE)
}

fn align_up(value: usize, align: usize) -> usize {
    let mask = align - 1;
    (value + mask) & !mask
}

fn align_up_u64(value: u64, align: u64) -> u64 {
    let mask = align - 1;
    (value + mask) & !mask
}

fn align_down_u64(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}
