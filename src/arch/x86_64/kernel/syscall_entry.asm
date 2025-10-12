; Syscall entry stub for Ares kernel

[global syscall_entry]
[extern syscall_trampoline]

syscall_entry:
    swapgs
    push rbp
    mov rbp, rsp

    push r11             ; saved rflags
    push rcx             ; return rip

    push rax             ; syscall number
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9

    mov rdi, rsp         ; pointer to frame
    call syscall_trampoline

    ; RAX already contains return value from dispatcher
    add rsp, 8*7         ; pop r9..rax
    pop rcx              ; restore return rip
    pop r11              ; restore rflags

    pop rbp
    swapgs
    sysret

section .note.GNU-stack
