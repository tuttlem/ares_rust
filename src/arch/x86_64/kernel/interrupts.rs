#![allow(dead_code)]

use core::mem::size_of;

use crate::klog;
use super::mmu;

type InterruptHandler = fn(&mut InterruptFrame);

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    fn set_handler(&mut self, handler: unsafe extern "C" fn(), selector: u16, type_attr: u8, ist: u8) {
        let addr = handler as u64;
        self.offset_low = addr as u16;
        self.selector = selector;
        self.ist = ist;
        self.type_attr = type_attr;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.zero = 0;
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

#[repr(C)]
pub struct InterruptFrame {
    pub ds: u64,
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub int_no: u64,
    pub err_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub user_rsp: u64,
    pub user_ss: u64,
}


pub mod irq {
    pub const PIT:           u8 = 0;  // Timer
    pub const KEYBOARD:      u8 = 1;
    pub const CASCADE:       u8 = 2;  // Slave PIC cascade
    pub const COM2:          u8 = 3;
    pub const COM1:          u8 = 4;
    pub const LPT2:          u8 = 5;
    pub const FLOPPY:        u8 = 6;
    pub const LPT1_SPURIOUS: u8 = 7;
    pub const RTC:           u8 = 8;
    pub const ACPI:          u8 = 9;
    pub const IRQ10:         u8 = 10;
    pub const IRQ11:         u8 = 11;
    pub const MOUSE:         u8 = 12;
    pub const FPU:           u8 = 13;
    pub const PRIMARY_IDE:   u8 = 14;
    pub const SECONDARY_IDE: u8 = 15;
}

pub const PIC_MASTER_OFFSET: u8 = 0x20;
pub const PIC_SLAVE_OFFSET:  u8 = 0x28;


pub mod vectors {
    use super::irq;

    // -------- CPU Exceptions (0x00–0x1F) --------
    pub const DIVIDE_ERROR:             u8 = 0;   // #DE
    pub const DEBUG:                    u8 = 1;   // #DB
    pub const NON_MASKABLE_INTERRUPT:   u8 = 2;   // NMI
    pub const BREAKPOINT:               u8 = 3;   // #BP (INT3)
    pub const OVERFLOW:                 u8 = 4;   // #OF (INTO)
    pub const BOUND_RANGE_EXCEEDED:     u8 = 5;   // #BR
    pub const INVALID_OPCODE:           u8 = 6;   // #UD
    pub const DEVICE_NOT_AVAILABLE:     u8 = 7;   // #NM (x87 not available)
    pub const DOUBLE_FAULT:             u8 = 8;   // #DF (errcode=0 always)
    pub const COPROCESSOR_SEG_OVERRUN:  u8 = 9;   // obsolete
    pub const INVALID_TSS:              u8 = 10;  // #TS
    pub const SEGMENT_NOT_PRESENT:      u8 = 11;  // #NP
    pub const STACK_SEGMENT_FAULT:      u8 = 12;  // #SS
    pub const GENERAL_PROTECTION:       u8 = 13;  // #GP
    pub const PAGE_FAULT:               u8 = 14;  // #PF (errcode bitfield)
    // 15 reserved
    pub const X87_FLOATING_POINT:       u8 = 16;  // #MF
    pub const ALIGNMENT_CHECK:          u8 = 17;  // #AC (ring3 only unless AM=1)
    pub const MACHINE_CHECK:            u8 = 18;  // #MC
    pub const SIMD_FLOAT_EXCEPTION:     u8 = 19;  // #XM/#XF (SSE/SSE2)
    pub const VIRTUALIZATION_EXCEPTION: u8 = 20;  // #VE (VT-x EPT)
    pub const CONTROL_PROTECTION:       u8 = 21;  // #CP (CET/IBT/ENDBR)
    // 22–31 reserved

    pub const PIT:           u8 = super::PIC_MASTER_OFFSET + irq::PIT;
    pub const KEYBOARD:      u8 = super::PIC_MASTER_OFFSET + irq::KEYBOARD;
    pub const CASCADE:       u8 = super::PIC_MASTER_OFFSET + irq::CASCADE;
    pub const COM2:          u8 = super::PIC_MASTER_OFFSET + irq::COM2;
    pub const COM1:          u8 = super::PIC_MASTER_OFFSET + irq::COM1;
    pub const LPT2:          u8 = super::PIC_MASTER_OFFSET + irq::LPT2;
    pub const FLOPPY:        u8 = super::PIC_MASTER_OFFSET + irq::FLOPPY;
    pub const LPT1_SPURIOUS: u8 = super::PIC_MASTER_OFFSET + irq::LPT1_SPURIOUS;
    pub const RTC:           u8 = super::PIC_SLAVE_OFFSET + (irq::RTC - 8);
    pub const ACPI:          u8 = super::PIC_SLAVE_OFFSET + (irq::ACPI - 8);
    pub const IRQ10:         u8 = super::PIC_SLAVE_OFFSET + (irq::IRQ10 - 8);
    pub const IRQ11:         u8 = super::PIC_SLAVE_OFFSET + (irq::IRQ11 - 8);
    pub const MOUSE:         u8 = super::PIC_SLAVE_OFFSET + (irq::MOUSE - 8);
    pub const FPU:           u8 = super::PIC_SLAVE_OFFSET + (irq::FPU - 8);
    pub const PRIMARY_IDE:   u8 = super::PIC_SLAVE_OFFSET + (irq::PRIMARY_IDE - 8);
    pub const SECONDARY_IDE: u8 = super::PIC_SLAVE_OFFSET + (irq::SECONDARY_IDE - 8);


    // -------- OS-defined software interrupts --------
    // Keep INT 0x80 for compatibility/testing even if you use SYSCALL/SYSRET.
    pub const INT_SYSCALL:   u8 = 0x80;

    // -------- IOAPIC/MSI general device range --------
    // When you move to IOAPIC/MSI, give yourself a wide, tidy block
    // for PCI/MSI/MSI-X devices to avoid clashing with exceptions and LAPIC.
    // Common choice: 0x40–0xEF (you assign per-device at init).
    pub const IOAPIC_BASE:   u8 = 0x40; // start of "external device" pool
    // (no fixed constants here; allocate dynamically from this range)

    // -------- Local APIC (per-CPU) vectors --------
    // These are *chosen by you* when programming the LAPIC LVT entries.
    // Carve out a top-of-space block so they never collide with IOAPIC/MSI.
    pub const LAPIC_TIMER:   u8 = 0xF0; // LVT Timer
    pub const LAPIC_THERMAL: u8 = 0xF1; // LVT Thermal Sensor (if supported)
    pub const LAPIC_PERF:    u8 = 0xF2; // LVT Performance Monitoring
    pub const LAPIC_LINT0:   u8 = 0xF3; // If you route ExtINT/NMI here (optional)
    pub const LAPIC_LINT1:   u8 = 0xF4; // Often NMI source (if used)
    pub const LAPIC_ERROR:   u8 = 0xFE; // LVT Error
    pub const SPURIOUS:      u8 = 0xFF; // LAPIC Spurious Vector (APIC_SPIV)

    // -------- Inter-Processor Interrupts (IPIs) --------
    // Reserve a small block for SMP housekeeping:
    pub const IPI_RESCHEDULE:    u8 = 0xF5; // scheduler poke
    pub const IPI_TLB_SHOOTDOWN: u8 = 0xF6; // flush TLB on peers
    pub const IPI_STOP_CPU:      u8 = 0xF7; // halt/park secondary CPUs
    // (Send with ICR in LAPIC/x2APIC; these are just the vectors they arrive on)

    // -------- Suggested policy (doc-only) --------
    //  00–1F: CPU exceptions (fixed)
    //  20–2F: Legacy PIC (if enabled)
    //  30–3F: Free (you can keep this empty as a buffer)
    //  40–EF: IOAPIC/MSI device pool (assign dynamically)
    //  80    : INT 0x80 syscall (optional compat)
    //  F0–FF: LAPIC + IPIs + spurious (per-CPU)
}

const IDT_ENTRIES: usize = 256;

static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::missing(); IDT_ENTRIES];
static mut HANDLERS: [InterruptHandler; IDT_ENTRIES] = [default_handler; IDT_ENTRIES];

#[link_section = ".data"]
static mut IDTR: Idtr = Idtr { limit: 0, base: 0 };

extern "C" {
    fn idt_stub_load(idtr: *const Idtr);

    fn isr_0();
    fn isr_1();
    fn isr_2();
    fn isr_3();
    fn isr_4();
    fn isr_5();
    fn isr_6();
    fn isr_7();
    fn isr_8();
    fn isr_9();
    fn isr_10();
    fn isr_11();
    fn isr_12();
    fn isr_13();
    fn isr_14();
    fn isr_15();
    fn isr_16();
    fn isr_17();
    fn isr_18();
    fn isr_19();
    fn isr_20();
    fn isr_21();
    fn isr_22();
    fn isr_23();
    fn isr_24();
    fn isr_25();
    fn isr_26();
    fn isr_27();
    fn isr_28();
    fn isr_29();
    fn isr_30();
    fn isr_31();

    fn irq_0();
    fn irq_1();
    fn irq_2();
    fn irq_3();
    fn irq_4();
    fn irq_5();
    fn irq_6();
    fn irq_7();
    fn irq_8();
    fn irq_9();
    fn irq_10();
    fn irq_11();
    fn irq_12();
    fn irq_13();
    fn irq_14();
    fn irq_15();
}

const GDT_KERNEL_CODE: u16 = 0x08;
const IDT_TYPE_ATTR: u8 = 0b1000_1110; // present, DPL=0, 64-bit interrupt gate

pub fn init() {
    unsafe {
        setup_idt();
        pic::remap(32, 40);
        load_idt();
    }

    klog::writeln("[interrupts] IDT loaded");
}

pub fn register_handler(vector: u8, handler: InterruptHandler) {
    unsafe {
        HANDLERS[vector as usize] = handler;
    }
}

pub fn enable() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

pub fn disable() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

pub fn enable_irq(line: u8) {
    unsafe { pic::unmask(line); }
}

pub fn disable_irq(line: u8) {
    unsafe { pic::mask(line); }
}

pub fn enable_vector(vector: u8) {
    if vector >= PIC_MASTER_OFFSET {
        enable_irq(vector - PIC_MASTER_OFFSET);
    }
}

pub fn disable_vector(vector: u8) {
    if vector >= PIC_MASTER_OFFSET {
        disable_irq(vector - PIC_MASTER_OFFSET);
    }
}

fn default_handler(frame: &mut InterruptFrame) {
    klog!("[interrupts] Unhandled vector {} err=0x{:X}\n", frame.int_no, frame.err_code);
}

fn page_fault_handler(frame: &mut InterruptFrame) {
    let fault_addr = unsafe { mmu::read_cr2() };
    let err = frame.err_code;

    let present = (err & 1) != 0;
    let write = (err & 2) != 0;
    let user = (err & 4) != 0;
    let reserved = (err & 8) != 0;
    let instruction = (err & 16) != 0;

    klog!(
        "[page_fault] addr=0x{:016X} err=0x{:X} rip=0x{:016X} cs=0x{:X} present={} write={} user={} reserved={} instruction={}\n",
        fault_addr,
        err,
        frame.rip,
        frame.cs,
        present,
        write,
        user,
        reserved,
        instruction
    );
}

fn general_protection_handler(frame: &mut InterruptFrame) {
    use crate::process;

    let pid = process::current_pid();
    klog!(
        "[gpf] pid={:?} rip=0x{:016X} cs=0x{:X} rflags=0x{:016X} rsp=0x{:016X} err=0x{:X}\n",
        pid,
        frame.rip,
        frame.cs,
        frame.rflags,
        frame.rsp,
        frame.err_code
    );

    if let Some(pid) = pid {
        if let Ok(()) = process::dump_process(pid) {
            klog!("[gpf] dumped process {}\n", pid);
        }
    }
}

fn ata_primary_irq(frame: &mut InterruptFrame) {
    use crate::process;
    let pid = process::current_pid();

    klog!(
        "[ata] pid={:?} rip=0x{:016X} cs=0x{:X} rflags=0x{:016X} rsp=0x{:016X} err=0x{:X}\n",
        pid,
        frame.rip,
        frame.cs,
        frame.rflags,
        frame.rsp,
        frame.err_code
    );

    unsafe {
        use crate::arch::x86_64::io::inb;
        let _status = inb(0x1F7); // clears the IRQ
    }
}

fn invalid_opcode_handler(frame: &mut InterruptFrame) {
    use crate::process;
    use core::slice;

    let pid = process::current_pid();
    let rip = frame.rip;
    let bytes = unsafe { slice::from_raw_parts(rip as *const u8, 16) };

    klog!(
        "[invop] pid={:?} rip=0x{:016X} cs=0x{:X} rflags=0x{:016X} bytes={:02X?}\n",
        pid,
        rip,
        frame.cs,
        frame.rflags,
        bytes
    );

    if let Some(pid) = pid {
        if let Ok(()) = process::dump_process(pid) {
            klog!("[invop] dumped process {}\n", pid);
        }
    }
}

#[no_mangle]
extern "C" fn isr_handler(frame: &mut InterruptFrame) {
    dispatch(frame);
}

#[no_mangle]
extern "C" fn irq_handler(frame: &mut InterruptFrame) {
    dispatch(frame);
    pic::send_eoi(frame.int_no as u8);
}

fn dispatch(frame: &mut InterruptFrame) {
    let vector = frame.int_no as usize;

    let handler = unsafe { HANDLERS[vector] };
    handler(frame);
}

unsafe fn setup_idt() {
    let isr_handlers: [unsafe extern "C" fn(); 32] = [
        isr_0, isr_1, isr_2, isr_3, isr_4, isr_5, isr_6, isr_7,
        isr_8, isr_9, isr_10, isr_11, isr_12, isr_13, isr_14, isr_15,
        isr_16, isr_17, isr_18, isr_19, isr_20, isr_21, isr_22, isr_23,
        isr_24, isr_25, isr_26, isr_27, isr_28, isr_29, isr_30, isr_31,
    ];

    let irq_handlers: [unsafe extern "C" fn(); 16] = [
        irq_0, irq_1, irq_2, irq_3, irq_4, irq_5, irq_6, irq_7,
        irq_8, irq_9, irq_10, irq_11, irq_12, irq_13, irq_14, irq_15,
    ];

    for (index, handler) in isr_handlers.iter().enumerate() {
        IDT[index].set_handler(*handler, GDT_KERNEL_CODE, IDT_TYPE_ATTR, 0);
    }

    register_handler(vectors::PAGE_FAULT, page_fault_handler);
    register_handler(vectors::GENERAL_PROTECTION, general_protection_handler);
    register_handler(vectors::INVALID_OPCODE, invalid_opcode_handler);

    register_handler(vectors::PRIMARY_IDE, ata_primary_irq);

    for (i, handler) in irq_handlers.iter().enumerate() {
        let index = 32 + i;
        IDT[index].set_handler(*handler, GDT_KERNEL_CODE, IDT_TYPE_ATTR, 0);
    }

    IDTR.limit = (size_of::<IdtEntry>() * IDT_ENTRIES - 1) as u16;
    IDTR.base = core::ptr::addr_of!(IDT) as u64;
}

unsafe fn load_idt() {
    idt_stub_load(core::ptr::addr_of!(IDTR));
}

mod pic {
    use super::klog;
    use crate::arch::x86_64::io::{inb, outb};

    const PIC1: u16 = 0x20;
    const PIC2: u16 = 0xA0;
    const PIC1_CMD: u16 = PIC1;
    const PIC1_DATA: u16 = PIC1 + 1;
    const PIC2_CMD: u16 = PIC2;
    const PIC2_DATA: u16 = PIC2 + 1;

    const PIC_EOI: u8 = 0x20;

    const ICW1_INIT: u8 = 0x10;
    const ICW1_ICW4: u8 = 0x01;
    const ICW4_8086: u8 = 0x01;

    static mut MASK_MASTER: u8 = 0xFF;
    static mut MASK_SLAVE: u8 = 0xFF;

    pub(super) unsafe fn remap(offset1: u8, offset2: u8) {
        let mask1 = inb(PIC1_DATA);
        let mask2 = inb(PIC2_DATA);

        outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
        outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);

        outb(PIC1_DATA, offset1);
        outb(PIC2_DATA, offset2);

        outb(PIC1_DATA, 0x04);
        outb(PIC2_DATA, 0x02);

        outb(PIC1_DATA, ICW4_8086);
        outb(PIC2_DATA, ICW4_8086);

        MASK_MASTER = mask1;
        MASK_SLAVE = mask2;

        outb(PIC1_DATA, MASK_MASTER);
        outb(PIC2_DATA, MASK_SLAVE);

        klog::writeln("[interrupts] PIC remapped");
    }

    pub(super) fn send_eoi(vector: u8) {
        unsafe {
            if vector >= 40 {
                outb(PIC2_CMD, PIC_EOI);
            }

            outb(PIC1_CMD, PIC_EOI);
        }
    }

    pub(super) unsafe fn mask(irq: u8) {
        if irq < 8 {
            MASK_MASTER |= 1 << irq;
            outb(PIC1_DATA, MASK_MASTER);
        } else {
            let line = irq - 8;
            MASK_SLAVE |= 1 << line;
            outb(PIC2_DATA, MASK_SLAVE);
        }
    }

    pub(super) unsafe fn unmask(irq: u8) {
        if irq < 8 {
            MASK_MASTER &= !(1 << irq);
            outb(PIC1_DATA, MASK_MASTER);
        } else {
            let line = irq - 8;
            MASK_SLAVE &= !(1 << line);
            outb(PIC2_DATA, MASK_SLAVE);
            // Ensure cascade line enabled on the master when using the slave PIC
            MASK_MASTER &= !(1 << 2);
            outb(PIC1_DATA, MASK_MASTER);
        }
    }

}
