; Minimal Linux i386 console input/output probe.
bits 32

section .text
global _start

_start:
    mov eax, 4              ; SYS_write
    mov ebx, 1              ; stdout
    mov ecx, listening
    mov edx, listening_end - listening
    int 0x80

    mov eax, 3              ; SYS_read
    xor ebx, ebx            ; stdin
    mov ecx, input
    mov edx, 1
    int 0x80

    mov eax, 4              ; SYS_write
    mov ebx, 1              ; stdout
    mov ecx, input
    mov edx, 1
    int 0x80

    mov eax, 4
    mov ebx, 1
    mov ecx, complete
    mov edx, complete_end - complete
    int 0x80

    mov eax, 1              ; SYS_exit
    xor ebx, ebx
    int 0x80

section .rodata
listening db "LINUX-ECHO-LISTENING", 13, 10
listening_end:
complete db "LINUX-ECHO-OK", 13, 10
complete_end:

section .bss
input resb 1
