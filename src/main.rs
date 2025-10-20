#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

#[macro_use]
extern crate alloc;

use uefi::prelude::*;
use x86_64::VirtAddr;

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
// mod usb_input; // Removed unused module
// mod graphics; // Removed unused module
// mod ahci; // Removed unused module
mod ahci;
#[cfg(feature = "alloc")]
mod ps2_keyboard;
#[cfg(feature = "alloc")]
mod rtl8139;
// mod ai_models; // Removed unused module
#[cfg(feature = "alloc")]
mod ai_models;
#[cfg(feature = "alloc")]
mod distributed_ai;
#[cfg(feature = "alloc")]
mod network_protocol;
#[cfg(feature = "alloc")]
mod kernel_protocol;
#[cfg(feature = "alloc")]
mod execution_journal;
#[cfg(feature = "alloc")]
mod semantic_fs;


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

        // Initialize PS/2 keyboard driver (optional)
        if let Err(e) = ps2_keyboard::init() {
            // PS/2 keyboard not available, continue without it
            serial_write(&alloc::format!("PS/2 keyboard init failed: {}", e));
        }

        // Initialize RTL8139 Ethernet controller
        if let Err(_) = rtl8139::init() {
            // RTL8139 not available, continue
        }

        // Initialize distributed AI coordinator with security manager
        let security_manager = crate::security::get_security_manager();
        let mut dai = distributed_ai::init(security_manager);
        
        // Initialize network protocol and bind to Ethernet if available
        #[cfg(feature = "alloc")]
        {
            let mut network_endpoint = network_protocol::init();
            // Try to bind to Ethernet controller
            if let Some(eth_controller) = ethernet::get_controller() {
                network_endpoint.bind_ethernet(eth_controller);
            }
            // Set network endpoint in distributed AI
            dai.set_network_endpoint(network_endpoint);
        }
        
        let dai_box = alloc::boxed::Box::new(dai);
        let dai_ptr = alloc::boxed::Box::into_raw(dai_box);
        unsafe {
            syscall::DISTRIBUTED_AI = dai_ptr;
        }

        // Initialize semantic filesystem
        let security_manager2 = crate::security::get_security_manager();
        let semantic_fs = semantic_fs::init(security_manager2);
        let semantic_fs_box = alloc::boxed::Box::new(semantic_fs);
        let semantic_fs_ptr = alloc::boxed::Box::into_raw(semantic_fs_box);
        unsafe {
            syscall::SEMANTIC_FS = semantic_fs_ptr;
        }

        // Initialize execution journal
        let mut execution_journal = crate::execution_journal::ExecutionJournal::new(None);
        if let Some(sm) = crate::security::get_security_manager() {
            execution_journal.set_security_manager(sm);
        }
        let execution_journal_box = alloc::boxed::Box::new(execution_journal);
        let execution_journal_ptr = alloc::boxed::Box::into_raw(execution_journal_box);
        unsafe {
            syscall::EXECUTION_JOURNAL = execution_journal_ptr;
        }
    }

    // For now, just return success
    uefi::Status::SUCCESS
}
