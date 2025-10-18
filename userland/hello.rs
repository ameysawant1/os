#![no_std]
#![no_main]

use core::arch::asm;

#[no_mangle]
pub extern "C" fn _start() {
    // Simple hello world using write syscall
    let message = b"Hello from userland!\n";
    let len = message.len();

    unsafe {
        asm!(
            "mov rax, 0",      // syscall number for write
            "mov rdi, 1",      // fd = stdout
            "mov rsi, {}",     // buf
            "mov rdx, {}",     // count
            "int 0x80",        // syscall
            in(reg) message.as_ptr(),
            in(reg) len,
        );
    }

    // Exit (for now, just loop)
    loop {}
}