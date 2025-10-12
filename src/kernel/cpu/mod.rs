#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
#[path = "../../arch/x86_64/kernel/cpuid.rs"]
mod arch;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("cpuid module is not implemented for this architecture");

pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

pub fn cpuid(eax: u32) -> CpuidResult {
    arch::cpuid(eax)
}

pub fn cpuid_ecx(eax: u32, ecx: u32) -> CpuidResult {
    arch::cpuid_ecx(eax, ecx)
}

pub fn highest_basic_leaf() -> u32 {
    cpuid(0).eax
}

pub fn highest_extended_leaf() -> u32 {
    cpuid(0x8000_0000).eax
}

pub fn vendor_string() -> [u8; 12] {
    let res = cpuid(0);
    let mut vendor = [0u8; 12];
    vendor[0..4].copy_from_slice(&res.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&res.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&res.ecx.to_le_bytes());
    vendor
}

pub struct Features {
    pub ecx: u32,
    pub edx: u32,
}

impl Features {
    pub fn has_ecx(&self, flag: u32) -> bool {
        self.ecx & flag != 0
    }

    pub fn has_edx(&self, flag: u32) -> bool {
        self.edx & flag != 0
    }
}

pub fn features() -> Features {
    let res = cpuid(1);
    Features {
        ecx: res.ecx,
        edx: res.edx,
    }
}

pub mod vendor {
    pub const OLD_AMD: &[u8; 12] = b"AMDisbetter!";
    pub const AMD: &[u8; 12] = b"AuthenticAMD";
    pub const INTEL: &[u8; 12] = b"GenuineIntel";
    pub const VIA: &[u8; 12] = b"CentaurHauls";
    pub const OLD_TRANSMETA: &[u8; 12] = b"TransmetaCPU";
    pub const TRANSMETA: &[u8; 12] = b"GenuineTMx86";
    pub const CYRIX: &[u8; 12] = b"CyrixInstead";
    pub const NEXGEN: &[u8; 12] = b"NexGenDriven";
    pub const UMC: &[u8; 12] = b"UMC UMC UMC ";
    pub const SIS: &[u8; 12] = b"SiS SiS SiS ";
    pub const NSC: &[u8; 12] = b"Geode by NSC";
    pub const RISE: &[u8; 12] = b"RiseRiseRise";
    pub const CENTAUR: &[u8; 12] = b"CentaurHauls";
}

pub mod feature {
    pub mod ecx {
        pub const SSE3: u32 = 1 << 0;
        pub const PCLMUL: u32 = 1 << 1;
        pub const DTES64: u32 = 1 << 2;
        pub const MONITOR: u32 = 1 << 3;
        pub const DS_CPL: u32 = 1 << 4;
        pub const VMX: u32 = 1 << 5;
        pub const SMX: u32 = 1 << 6;
        pub const EST: u32 = 1 << 7;
        pub const TM2: u32 = 1 << 8;
        pub const SSSE3: u32 = 1 << 9;
        pub const CID: u32 = 1 << 10;
        pub const FMA: u32 = 1 << 12;
        pub const CX16: u32 = 1 << 13;
        pub const ETPRD: u32 = 1 << 14;
        pub const PDCM: u32 = 1 << 15;
        pub const DCA: u32 = 1 << 18;
        pub const SSE4_1: u32 = 1 << 19;
        pub const SSE4_2: u32 = 1 << 20;
        pub const X2APIC: u32 = 1 << 21;
        pub const MOVBE: u32 = 1 << 22;
        pub const POPCNT: u32 = 1 << 23;
        pub const AES: u32 = 1 << 25;
        pub const XSAVE: u32 = 1 << 26;
        pub const OSXSAVE: u32 = 1 << 27;
        pub const AVX: u32 = 1 << 28;
    }

    pub mod edx {
        pub const FPU: u32 = 1 << 0;
        pub const VME: u32 = 1 << 1;
        pub const DE: u32 = 1 << 2;
        pub const PSE: u32 = 1 << 3;
        pub const TSC: u32 = 1 << 4;
        pub const MSR: u32 = 1 << 5;
        pub const PAE: u32 = 1 << 6;
        pub const MCE: u32 = 1 << 7;
        pub const CX8: u32 = 1 << 8;
        pub const APIC: u32 = 1 << 9;
        pub const SEP: u32 = 1 << 11;
        pub const MTRR: u32 = 1 << 12;
        pub const PGE: u32 = 1 << 13;
        pub const MCA: u32 = 1 << 14;
        pub const CMOV: u32 = 1 << 15;
        pub const PAT: u32 = 1 << 16;
        pub const PSE36: u32 = 1 << 17;
        pub const PSN: u32 = 1 << 18;
        pub const CLF: u32 = 1 << 19;
        pub const DTES: u32 = 1 << 21;
        pub const ACPI: u32 = 1 << 22;
        pub const MMX: u32 = 1 << 23;
        pub const FXSR: u32 = 1 << 24;
        pub const SSE: u32 = 1 << 25;
        pub const SSE2: u32 = 1 << 26;
        pub const SS: u32 = 1 << 27;
        pub const HTT: u32 = 1 << 28;
        pub const TM1: u32 = 1 << 29;
        pub const IA64: u32 = 1 << 30;
        pub const PBE: u32 = 1 << 31;
    }
}
