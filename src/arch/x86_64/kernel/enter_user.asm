[global enter_user_mode]

; void enter_user_mode(u64 entry, u64 user_rsp)
; The caller provides entry in r15 and rsp in r14 via context switch.

enter_user_mode:
    mov rax, r15         ; user entry
    mov rdx, r14         ; user stack top

    mov bx, 0x23
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx

    mov rcx, 0x202       ; RFLAGS with IF set

    push 0x23            ; user SS
    push rdx             ; user RSP
    push rcx             ; RFLAGS
    push 0x1B            ; user CS
    push rax             ; user RIP

    iretq

section .note.GNU-stack
