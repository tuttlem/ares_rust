#![allow(bad_asm_style)]
use core::arch::global_asm;

global_asm!(r#"
    .intel_syntax noprefix
    .section .text

    .globl enter_user_mode
    .type enter_user_mode, @function
enter_user_mode:
    mov rax, r15
    mov rdx, r14

    mov bx, 0x23
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx

    mov rcx, 0x202

    push 0x23
    push rdx
    push rcx
    push 0x1B
    push rax

    iretq

    .section .note.GNU-stack,"",@progbits
"#);
