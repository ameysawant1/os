#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi_services::println;
use uefi::table::boot::MemoryType;

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    println!("Hello from Rust UEFI OS!");
    println!("Bootloader initialized successfully.");
    println!("Preparing kernel hand-off...");

    // Get memory map before exiting boot services
    let mut memory_map_buf = [0u8; 4096 * 4];
    let memory_map = match system_table.boot_services().memory_map(&mut memory_map_buf) {
        Ok(map) => map,
        Err(e) => {
            println!("Failed to get memory map: {:?}", e);
            return Status::ABORTED;
        }
    };

    println!("Memory map acquired ({} entries)", memory_map.entries().len());
    println!("About to call exit_boot_services - this is the critical transition!");
    println!("After this call, UEFI boot services will be unavailable...");

    // CRITICAL: Exit boot services - this is the kernel hand-off!
    // Note: After this call, UEFI boot services are no longer available
    // This transitions us from UEFI application to bare-metal kernel
    println!("Calling exit_boot_services...");
    println!("If successful, this will be the last UEFI message you see!");

    // Exit boot services - this either succeeds or resets the system
    let (_runtime_table, _final_memory_map) = system_table.exit_boot_services(MemoryType::LOADER_DATA);

    // SUCCESS! We're now in bare-metal kernel mode
    // UEFI services are gone - we can't use println! anymore
    // Set up basic VGA text output for kernel messages

    // VGA text buffer is at 0xB8000 in memory
    const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
    const VGA_WIDTH: usize = 80;
    const VGA_HEIGHT: usize = 25;

    // Clear screen and set up basic text output
    unsafe {
        for i in 0..(VGA_WIDTH * VGA_HEIGHT) {
            VGA_BUFFER.add(i).write_volatile(0x0F00); // White on black space
        }
    }

    // Write kernel initialization message
    let message = b"Kernel initialized! Running in bare-metal mode...";
    unsafe {
        for (i, &byte) in message.iter().enumerate() {
            if i < VGA_WIDTH {
                let char = (byte as u16) | 0x0F00; // White on black
                VGA_BUFFER.add(i).write_volatile(char);
            }
        }
    }

    // For now, just infinite loop to show we're still running
    // (In a real kernel, this would be the scheduler/main kernel loop)
    loop {
        // Busy wait - in real kernel we'd have interrupts/timers
        unsafe {
            core::arch::asm!("pause");
        }
    }
}