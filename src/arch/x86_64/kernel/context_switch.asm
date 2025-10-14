[global context_switch]
[global preempt_trampoline]

; void context_switch(Context* current, const Context* next)
; rdi = current, rsi = next

context_switch:
    ; Save current callee-saved registers and state
    mov [rdi + 0x00], r15
    mov [rdi + 0x08], r14
    mov [rdi + 0x10], r13
    mov [rdi + 0x18], r12
    mov [rdi + 0x20], rbx
    mov [rdi + 0x28], rbp
    mov [rdi + 0x30], rsp

    lea rax, [rel .return_point]
    mov [rdi + 0x38], rax
    pushfq
    pop QWORD [rdi + 0x40]

    ; Load next context
    mov r15, [rsi + 0x00]
    mov r14, [rsi + 0x08]
    mov r13, [rsi + 0x10]
    mov r12, [rsi + 0x18]
    mov rbx, [rsi + 0x20]
    mov rbp, [rsi + 0x28]
    mov rsp, [rsi + 0x30]
    mov rax, [rsi + 0x40]
    push rax
    popfq

    mov rax, [rsi + 0x38]
    jmp rax

.return_point:
    ret

[extern preempt_do_switch]

preempt_trampoline:
    call preempt_do_switch
    jmp rax

section .note.GNU-stack
