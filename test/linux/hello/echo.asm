; Minimal Linux i386 console-output probe.
; Exercises the Linux personality's write syscall and console output route.
bits 32

section .text
global _start

_start:
    mov eax, 4              ; SYS_write
    mov ebx, 1              ; stdout
    mov ecx, message
    mov edx, message_end - message
    int 0x80

    mov eax, 1              ; SYS_exit
    xor ebx, ebx
    int 0x80

section .rodata
message db "Hello from Linux personality", 13, 10
message_end:
