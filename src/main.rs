#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

#[macro_use]
extern crate alloc;

use uefi::prelude::*;
use x86_64::{PhysAddr, VirtAddr};
use x86_64::structures::paging::PageTableFlags;

// Add new modules
mod syscall;
mod process;
mod frame_allocator;
mod filesystem;
mod security;
mod heap_allocator;
mod virtual_memory;
mod scheduler;
mod usb;
mod apic;
mod pci;
mod ethernet;
mod usb_input;
mod graphics;
mod ahci;
#[cfg(feature = "alloc")]
mod ai_models;
#[cfg(feature = "alloc")]
mod distributed_ai;
#[cfg(feature = "alloc")]
mod network_protocol;
#[cfg(feature = "alloc")]
mod execution_journal;


/// Simple serial write function for debugging (stub for now)
pub fn serial_write(_s: &str) {
    // TODO: Implement actual serial output or UEFI console output
}

/// Legacy PIC for interrupt handling (fallback)
pub static mut PICS: pic8259::ChainedPics = unsafe { pic8259::ChainedPics::new(32, 40) };

#[entry]
fn efi_main() -> Status {
    // Initialize security manager
    crate::security::init();

    #[cfg(feature = "alloc")]
    {
        // Initialize frame allocator (simplified)
        crate::frame_allocator::init();

        // Initialize heap allocator
        let heap_start = VirtAddr::new(0x10_0000); // 1MB, should be identity mapped by UEFI
        let heap_size = 100 * 1024; // 100 KiB
        crate::heap_allocator::init_heap_with_pages(heap_start.as_u64() as usize, heap_size)
            .expect("Heap initialization failed");

        // Initialize virtual memory (use UEFI's identity mapping)
        // Skip our own virtual memory manager since UEFI already provides identity mapping

        // Initialize execution journal
        crate::execution_journal::init();

        // Initialize distributed AI coordinator with security manager
        let security_manager = crate::security::get_security_manager();
        let dai = distributed_ai::init(security_manager);
        let dai_box = alloc::boxed::Box::new(dai);
        let dai_ptr = alloc::boxed::Box::into_raw(dai_box);
        unsafe {
            syscall::DISTRIBUTED_AI = dai_ptr;
        }
    }

    // For now, just return success
    uefi::Status::SUCCESS
}
