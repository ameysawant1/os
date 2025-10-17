#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi_services::println;

#[entry]
fn main(_image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    println!("Hello from Rust UEFI OS!");
    println!("Bootloader initialized successfully.");

    // Simple kernel-like functionality
    println!("Kernel starting...");

    // For now, just loop forever
    loop {
        // In a real kernel, we'd have scheduling, interrupts, etc.
        // For demo purposes, we'll just print a message periodically
        println!("OS running...");

        // Wait for 1 second
        system_table.boot_services().stall(1_000_000);
    }
}