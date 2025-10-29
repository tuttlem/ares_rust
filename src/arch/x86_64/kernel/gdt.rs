use core::arch::asm;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

const KERNEL_CODE: u64 = 0x00A0_9A00_0000_0000;
const KERNEL_DATA: u64 = 0x00A0_9200_0000_0000;
const USER_CODE: u64 = 0x00A0_FA00_0000_0000;
const USER_DATA: u64 = 0x00A0_F200_0000_0000;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_CODE_SELECTOR: u16 = 0x18;
pub const USER_DATA_SELECTOR: u16 = 0x20;
const TSS_SELECTOR: u16 = 0x28;

#[repr(C, packed)]
struct Gdtr {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    _reserved0: u32,
    rsp: [u64; 3],
    _reserved1: u64,
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    iomap_base: u16,
}

impl TaskStateSegment {
    const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp: [0; 3],
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: size_of::<Self>() as u16,
        }
    }
}

static INITIALISED: AtomicBool = AtomicBool::new(false);

const GDT_LEN: usize = 7;

static mut GDT: [u64; GDT_LEN] = [
    0,
    KERNEL_CODE,
    KERNEL_DATA,
    USER_CODE,
    USER_DATA,
    0,
    0,
];

static mut GDTR: Gdtr = Gdtr { limit: 0, base: 0 };

#[repr(C, align(16))]
struct AlignedTss(TaskStateSegment);

static mut TSS: AlignedTss = AlignedTss(TaskStateSegment::new());

pub fn init() {
    if INITIALISED.swap(true, Ordering::AcqRel) {
        return;
    }

    unsafe {
        encode_tss_descriptor();

        GDTR.limit = (GDT_LEN * size_of::<u64>() - 1) as u16;
        GDTR.base = ptr::addr_of!(GDT) as u64;

        asm!("lgdt [{0}]", in(reg) ptr::addr_of!(GDTR), options(readonly, nostack));
        asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nostack));
    }
}

pub fn set_kernel_stack(stack_top: u64) {
    unsafe {
        TSS.0.rsp[0] = stack_top;
    }
}

fn encode_tss_descriptor() {
    unsafe {
        let tss_ptr = ptr::addr_of!(TSS.0);
        let base = tss_ptr as u64;
        let limit = (size_of::<TaskStateSegment>() - 1) as u32;

        let base_low = base & 0xFFFF;
        let base_mid = (base >> 16) & 0xFF;
        let base_high = (base >> 24) & 0xFF;
        let base_upper = (base >> 32) & 0xFFFF_FFFF;

        let limit_low = (limit & 0xFFFF) as u64;
        let limit_high = ((limit >> 16) & 0xF) as u64;

        let mut lower = 0u64;
        lower |= limit_low;
        lower |= base_low << 16;
        lower |= base_mid << 32;
        lower |= (0x89u64) << 40; // type=0x9 (available 64-bit TSS), present
        lower |= limit_high << 48;
        lower |= (base_high) << 56;

        let upper = base_upper;

        GDT[5] = lower;
        GDT[6] = upper;
    }
}
