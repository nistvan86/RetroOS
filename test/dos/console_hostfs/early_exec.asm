; Early-console HostFS execution probe.
; Prints a marker through DOS and exits successfully.
org 0x100

    mov ah, 0x09
    mov dx, marker
    int 0x21
    mov ax, 0x4C00
    int 0x21

marker db "EARLY-EXEC-HOSTFS-OK", 13, 10, '$'
