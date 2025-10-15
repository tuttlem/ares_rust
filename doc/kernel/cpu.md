# CPU Feature Detection

Source: `src/arch/x86_64/kernel/cpu/` exposed through `src/kernel/cpu/mod.rs`.

## Capabilities

- Wraps the CPUID instruction to query vendor strings, supported features, and the highest basic/extended leaves.
- Provides compile-time constants for commonly interesting feature bits (SSE, AVX, etc.).
- Exposes convenience helpers used during boot for logging and capability checks.

## Key functions

| Function | Purpose |
|----------|---------|
| `cpuid(eax)` / `cpuid_ecx(eax, ecx)` | Issue raw CPUID calls, returning a `CpuidResult`. |
| `highest_basic_leaf()` / `highest_extended_leaf()` | Discover available CPUID leaves. |
| `vendor_string()` | Returns the 12-byte vendor ASCII string. |
| `features()` | Captures the `ecx`/`edx` feature words for leaf 1. |

The `feature::ecx` and `feature::edx` modules enumerate bit masks so subsystems can gate functionality on CPU support (e.g., SSE/SSE2 logging in `kmain`).

## Usage guidelines

- Always check the relevant feature bit before relying on instructions that may trap on older hardware.
- Fetch the vendor string once early during boot; it is cheap but does not change at runtime.
- Extend the feature tables if you need to detect additional capabilities.
