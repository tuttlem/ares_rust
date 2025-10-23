#![allow(bad_asm_style)]
use core::arch::global_asm;

global_asm!(r#"
    .intel_syntax noprefix
    .section .text

    .extern syscall_trampoline

    .globl syscall_entry
    .type syscall_entry, @function
syscall_entry:
    swapgs
    push rbp
    mov rbp, rsp

    push r11
    push rcx

    push rax
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9

    mov rdi, rsp
    call syscall_trampoline

    add rsp, 8*7
    pop rcx
    pop r11

    pop rbp
    swapgs
    sysret

    .section .note.GNU-stack,"",@progbits
"#);
