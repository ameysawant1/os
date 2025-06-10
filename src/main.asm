org 0x7C00
bits 16

start:
    ; Set text mode
    mov ax, 0x0003
    int 0x10

    ; Print message
    mov si, msg
    call print_string
    
    ; Infinite loop
    jmp $

print_string:
    lodsb
    or al, al
    jz .done
    mov ah, 0x0E
    int 0x10
    jmp print_string
.done:
    ret

msg db "Hello World!", 0

times 510-($-$$) db 0
dw 0xAA55