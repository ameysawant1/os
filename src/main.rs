#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

#[macro_use]
extern crate alloc;

use uefi::prelude::*;
use uefi::Identify;
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
mod utils;


/// Display a boot logo on the screen
fn display_boot_logo() -> Result<(), uefi::Status> {
    crate::utils::serial_write("Displaying boot logo...\n");
    let st_raw = uefi::table::system_table_raw().unwrap();
    unsafe {
        let st = &*st_raw.as_ptr();
        let boot_services = &*st.boot_services;
        let gop_guid = uefi::proto::console::gop::GraphicsOutput::GUID;
        let mut gop_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
        let status = (boot_services.locate_protocol)(&gop_guid, core::ptr::null_mut(), &mut gop_ptr);
        if status != uefi::Status::SUCCESS {
            crate::utils::serial_write("Failed to locate GOP\n");
            return Err(status);
        }
        crate::utils::serial_write("GOP located\n");
        let gop = &mut *(gop_ptr as *mut uefi_raw::protocol::console::GraphicsOutputProtocol);
        let mode = &*gop.mode;
        let info = &*mode.info;
        let width = info.horizontal_resolution as usize;
        let height = info.vertical_resolution as usize;
        let fb_base = mode.frame_buffer_base as *mut u32;
        crate::utils::serial_write(&alloc::format!("Resolution: {}x{}\n", width, height));
        // Clear screen to black
        for i in 0..(width * height) {
            *fb_base.add(i) = 0x00000000;
        }
        // Draw a simple logo - a colored rectangle
        let logo_width = 400;
        let logo_height = 200;
        let logo_x = (width - logo_width) / 2;
        let logo_y = (height - logo_height) / 2;
        // Draw blue background
        let blue = 0x00FF0000; // BGRA format
        for y in logo_y..logo_y + logo_height {
            for x in logo_x..logo_x + logo_width {
                let offset = y * width + x;
                *fb_base.add(offset) = blue;
            }
        }
        // Draw white border
        let white = 0xFFFFFFFF;
        for x in logo_x..logo_x + logo_width {
            *fb_base.add(logo_y * width + x) = white;
            *fb_base.add((logo_y + logo_height - 1) * width + x) = white;
        }
        for y in logo_y..logo_y + logo_height {
            *fb_base.add(y * width + logo_x) = white;
            *fb_base.add(y * width + logo_x + logo_width - 1) = white;
        }
    }
    crate::utils::serial_write("Boot logo displayed\n");
    Ok(())
}

/// Simple text drawing function (very basic)
// fn draw_text(fb: &mut FrameBuffer, width: usize, x: usize, y: usize, text: &str, color: u32) {
//     let mut current_x = x;
//     for ch in text.chars() {
//         if ch == ' ' {
//             current_x += 20;
//             continue;
//         }
//         // Very simple character rendering - just draw a rectangle for each character
//         for dy in 0..20 {
//             for dx in 0..10 {
//                 let px = current_x + dx;
//                 let py = y + dy;
//                 if px < width {
//                     let offset = (py * width + px) * 4;
//                     if offset + 3 < fb.size() {
//                         unsafe {
//                             fb.write_value(offset, color);
//                         }
//                     }
//                 }
//             }
//         }
//         current_x += 15;
//     }
// }

/// Legacy PIC for interrupt handling (fallback)
pub static mut PICS: pic8259::ChainedPics = unsafe { pic8259::ChainedPics::new(32, 40) };

#[entry]
fn efi_main() -> Status {
    // let st = unsafe { SYSTEM_TABLE_REF.unwrap() };

    // Display boot logo
    if let Err(_) = display_boot_logo() {
        // Logo display failed, continue
    }

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
            crate::utils::serial_write(&alloc::format!("PS/2 keyboard init failed: {}", e));
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
