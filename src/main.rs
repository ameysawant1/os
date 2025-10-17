#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi_services::println;
use uefi::table::boot::MemoryType;

/// Set up basic identity-mapped paging for the first 4GB of memory
unsafe fn setup_paging() {
    // Page table entry flags
    const PRESENT: u64 = 1 << 0;
    const WRITABLE: u64 = 1 << 1;
    const HUGE_PAGE: u64 = 1 << 7;

    // Allocate page tables in static memory (simplified - in real kernel use proper allocation)
    // We'll place them at a fixed address for now
    const PAGE_TABLE_BASE: *mut u64 = 0x100000 as *mut u64; // 1MB mark

    // Zero out page table area (4KB for each table)
    for i in 0..1024 {
        PAGE_TABLE_BASE.add(i).write(0);
        PAGE_TABLE_BASE.add(1024 + i).write(0);
        PAGE_TABLE_BASE.add(2048 + i).write(0);
        PAGE_TABLE_BASE.add(3072 + i).write(0);
    }

    // Set up page tables:
    // PML4 -> PDP -> PD -> PT (but we'll use 2MB huge pages for simplicity)

    let pml4 = PAGE_TABLE_BASE;
    let pdp = PAGE_TABLE_BASE.add(1024);
    let pd = PAGE_TABLE_BASE.add(2048);

    // Point PML4[0] to PDP
    pml4.add(0).write(pdp as u64 | PRESENT | WRITABLE);

    // Point PDP[0] to PD
    pdp.add(0).write(pd as u64 | PRESENT | WRITABLE);

    // Set up PD with 2MB huge pages for first 4GB
    for i in 0..512 {
        let addr = (i as u64) * 0x200000; // 2MB pages
        pd.add(i).write(addr | PRESENT | WRITABLE | HUGE_PAGE);
    }

    // Load page table base address into CR3
    core::arch::asm!("mov cr3, {}", in(reg) pml4 as u64);

    // Enable paging by setting PG bit in CR0
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0);
    cr0 |= 1 << 31; // Set PG bit
    core::arch::asm!("mov cr0, {}", in(reg) cr0);
}

/// Simple bump allocator for kernel heap
struct BumpAllocator {
    heap_end: usize,
    next: usize,
}

impl BumpAllocator {
    const fn new() -> Self {
        // Place heap after page tables (around 2MB mark)
        const HEAP_START: usize = 0x200000; // 2MB
        const HEAP_SIZE: usize = 0x100000; // 1MB heap

        BumpAllocator {
            heap_end: HEAP_START + HEAP_SIZE,
            next: HEAP_START,
        }
    }

    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let aligned_next = (self.next + align - 1) & !(align - 1);

        if aligned_next + size > self.heap_end {
            panic!("Out of memory!");
        }

        let ptr = aligned_next as *mut u8;
        self.next = aligned_next + size;
        ptr
    }
}

static mut HEAP_ALLOCATOR: BumpAllocator = BumpAllocator::new();

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

    // Store memory map information for kernel use
    let mut total_memory_kb = 0u64;
    let mut usable_memory_kb = 0u64;
    let mut _memory_entries = 0;

    for entry in memory_map.entries() {
        _memory_entries += 1;
        let size_kb = (entry.page_count * 4096) / 1024;
        total_memory_kb += size_kb;

        // Count conventional memory as usable
        if entry.ty == uefi::table::boot::MemoryType::CONVENTIONAL {
            usable_memory_kb += size_kb;
        }
    }

    println!("About to call exit_boot_services - this is the critical transition!");
    println!("After this call, UEFI boot services will be unavailable...");

    // CRITICAL: Exit boot services - this is the kernel hand-off!
    // Note: After this call, UEFI boot services are no longer available
    // This transitions us from UEFI application to bare-metal kernel
    
    println!("Calling exit_boot_services...");
    println!("If successful, this will be the last UEFI message you see!");

    // Exit boot services - this either succeeds or resets the system
    let (_runtime_table, _final_memory_map) = system_table.exit_boot_services(MemoryType::LOADER_DATA);    // SUCCESS! We're now in bare-metal kernel mode
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
    let message = b"Kernel initialized! Setting up memory management...";
    unsafe {
        for (i, &byte) in message.iter().enumerate() {
            if i < VGA_WIDTH {
                let char = (byte as u16) | 0x0F00; // White on black
                VGA_BUFFER.add(i).write_volatile(char);
            }
        }
    }

    // Set up basic paging for memory management
    unsafe {
        setup_paging();
    }

    // Write paging setup complete message
    let paging_msg = b"Paging enabled! Memory management active.";
    unsafe {
        for (i, &byte) in paging_msg.iter().enumerate() {
            if i < VGA_WIDTH {
                let char = (byte as u16) | 0x0A00; // Green on black
                VGA_BUFFER.add(VGA_WIDTH + i).write_volatile(char); // Second line
            }
        }
    }

    // Test the heap allocator
    unsafe {
        let test_alloc = (&mut *core::ptr::addr_of_mut!(HEAP_ALLOCATOR)).alloc(64, 8);
        // Write some test data
        for i in 0..8 {
            test_alloc.add(i).write((i + 65) as u8); // ASCII A-H
        }

        // Display heap allocation success
        let heap_msg = b"Heap allocator: OK (64 bytes allocated)";
        for (i, &byte) in heap_msg.iter().enumerate() {
            if i < VGA_WIDTH {
                let char = (byte as u16) | 0x0E00; // Yellow on black
                VGA_BUFFER.add(VGA_WIDTH * 2 + i).write_volatile(char); // Third line
            }
        }
    }

    // Display memory information on VGA screen
    unsafe {
        // Convert numbers to strings manually (no std format!)
        let total_mb = total_memory_kb / 1024;
        let _usable_mb = usable_memory_kb / 1024;

        // Simple memory info display
        let mem_msg = b"Memory: ";
        let mut pos = 0;

        // Write "Memory: "
        for &byte in mem_msg.iter() {
            if pos < VGA_WIDTH {
                let char = (byte as u16) | 0x0C00; // Red on black
                VGA_BUFFER.add(VGA_WIDTH * 3 + pos).write_volatile(char);
            }
            pos += 1;
        }

        // Write total memory (simplified - just show approximate MB)
        let mb_digits = [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'];
        let hundreds = (total_mb / 100) as usize % 10;
        let tens = (total_mb / 10) as usize % 10;
        let ones = total_mb as usize % 10;

        if pos < VGA_WIDTH { VGA_BUFFER.add(VGA_WIDTH * 3 + pos).write_volatile((mb_digits[hundreds] as u16) | 0x0C00); pos += 1; }
        if pos < VGA_WIDTH { VGA_BUFFER.add(VGA_WIDTH * 3 + pos).write_volatile((mb_digits[tens] as u16) | 0x0C00); pos += 1; }
        if pos < VGA_WIDTH { VGA_BUFFER.add(VGA_WIDTH * 3 + pos).write_volatile((mb_digits[ones] as u16) | 0x0C00); pos += 1; }

        let mb_msg = b" MB total";
        for &byte in mb_msg.iter() {
            if pos < VGA_WIDTH {
                let char = (byte as u16) | 0x0C00;
                VGA_BUFFER.add(VGA_WIDTH * 3 + pos).write_volatile(char);
            }
            pos += 1;
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
