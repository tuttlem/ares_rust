use core::arch::x86_64::__cpuid_count;

pub(super) fn cpuid(eax: u32) -> super::CpuidResult {
    cpuid_ecx(eax, 0)
}

pub(super) fn cpuid_ecx(eax: u32, ecx: u32) -> super::CpuidResult {
    let res = unsafe { __cpuid_count(eax, ecx) };

    super::CpuidResult {
        eax: res.eax,
        ebx: res.ebx,
        ecx: res.ecx,
        edx: res.edx,
    }
}
