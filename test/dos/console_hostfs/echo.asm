; Early-console DOS runtime input probe.
; Polls INT 16h until a key arrives, echoes it through DOS, prints a
; completion marker, and exits. Polling keeps the DOS thread runnable so the
; kernel event loop can deliver the serial scancode.
org 0x100

    mov ah, 0x09
    mov dx, listening
    int 0x21

wait_key:
    mov ah, 0x01
    int 0x16
    jz wait_key

    mov dl, al
    mov ah, 0x02
    int 0x21

    mov ah, 0x09
    mov dx, marker
    int 0x21

    mov ax, 0x4C00
    int 0x21

listening db "DOS-ECHO-LISTENING", 13, 10, '$'
marker db "DOS-ECHO-OK", 13, 10, '$'
