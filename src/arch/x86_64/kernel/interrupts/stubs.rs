#![allow(bad_asm_style)]
use core::arch::global_asm;

global_asm!(r#"
    .intel_syntax noprefix
    .section .text

    .macro push_all
        push rax
        push rcx
        push rdx
        push rbx
        mov rax, rsp
        push rax
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
    .endm

    .macro pop_all
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rax
        mov rsp, rax
        pop rbx
        pop rdx
        pop rcx
        pop rax
    .endm

    .macro isr_noerr num
        .globl isr_\num
        .type isr_\num, @function
    isr_\num:
        cli
        push 0
        push \num
        jmp isr_common
    .endm

    .macro isr_err num
        .globl isr_\num
        .type isr_\num, @function
    isr_\num:
        cli
        push \num
        jmp isr_common
    .endm

    .macro irq idx, vector
        .globl irq_\idx
        .type irq_\idx, @function
    irq_\idx:
        cli
        push 0
        push \vector
        jmp irq_common
    .endm

    .globl idt_stub_load
    .type idt_stub_load, @function
idt_stub_load:
    lidt [rdi]
    ret

    isr_noerr 0
    isr_noerr 1
    isr_noerr 2
    isr_noerr 3
    isr_noerr 4
    isr_noerr 5
    isr_noerr 6
    isr_noerr 7
    isr_err   8
    isr_noerr 9
    isr_err   10
    isr_err   11
    isr_err   12
    isr_err   13
    isr_err   14
    isr_noerr 15
    isr_noerr 16
    isr_noerr 17
    isr_noerr 18
    isr_noerr 19
    isr_noerr 20
    isr_noerr 21
    isr_noerr 22
    isr_noerr 23
    isr_noerr 24
    isr_noerr 25
    isr_noerr 26
    isr_noerr 27
    isr_noerr 28
    isr_noerr 29
    isr_noerr 30
    isr_noerr 31

    irq       0,  32
    irq       1,  33
    irq       2,  34
    irq       3,  35
    irq       4,  36
    irq       5,  37
    irq       6,  38
    irq       7,  39
    irq       8,  40
    irq       9,  41
    irq      10,  42
    irq      11,  43
    irq      12,  44
    irq      13,  45
    irq      14,  46
    irq      15,  47

    .globl isr_common
    .type isr_common, @function
isr_common:
    push_all

    mov ax, ds
    push rax

    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rdi, rsp
    call isr_handler

    pop rbx
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx

    pop_all

    add rsp, 16

    sti
    iretq

    .globl irq_common
    .type irq_common, @function
irq_common:
    push_all

    mov ax, ds
    push rax

    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rdi, rsp
    call irq_handler

    pop rbx
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx

    pop_all

    add rsp, 16

    sti
    iretq

    .section .note.GNU-stack,"",@progbits
"#);
