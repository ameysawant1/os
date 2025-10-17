#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi_services::println;

#[entry]
fn main(_image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    println!("Hello from Rust UEFI OS!");
    println!("Bootloader initialized successfully.");
    println!("Kernel starting...");

    let mut counter = 0;

    // Simple kernel-like functionality
    loop {
        counter += 1;
        println!("OS running... (iteration {})", counter);

        // Every 10 iterations, show some system info
        if counter % 10 == 0 {
            println!("System uptime: {} seconds", counter);
            println!("Memory status: Available");
        }

        // Wait for 1 second
        system_table.boot_services().stall(1_000_000);
    }
}