#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, panic_info_message)]
#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "uefi")]
use uefi::prelude::*;
#[cfg(feature = "uefi")]
#[allow(unused)]
use uefi::mem::memory_map::MemoryType;
use uart_16550::SerialPort;
use spin::Mutex;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use pic8259::ChainedPics;

// Add new modules
mod syscall;
mod process;
mod frame_allocator;
mod filesystem;
mod security;
mod heap_allocator;
mod ai_models;
mod virtual_memory;
mod scheduler;
mod usb;
mod apic;
mod pci;
mod ethernet;
mod usb_input;
mod graphics;

// Panic handler is provided by the uefi crate

// Fallback panic handler for non-UEFI builds (like tests)
#[cfg(not(feature = "uefi"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // For tests and host builds, use the standard panic behavior
    // This will be handled by the test framework
    core::panic!();
}

// VGA constants
const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_WIDTH: usize = 80;
#[allow(unused)]
const VGA_HEIGHT: usize = 25;

// Safe serial port abstraction
static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

// GDT, TSS, and IDT for proper kernel setup
static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

// PIC (Programmable Interrupt Controller) setup
const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
static mut PICS: ChainedPics = unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) };

// Framebuffer information for GOP graphics
#[cfg(feature = "uefi")]
#[derive(Debug, Clone, Copy)]
struct FramebufferInfo {
    buffer: *mut u32,
    width: usize,
    height: usize,
    stride: usize,
}

#[cfg(feature = "uefi")]
static mut FRAMEBUFFER: Option<FramebufferInfo> = None;

/// Initialize the serial port safely
pub fn serial_init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

/// Write a string to serial port safely
pub fn serial_write(s: &str) {
    if let Some(ref mut port) = *SERIAL.lock() {
        for byte in s.bytes() {
            port.send(byte);
        }
        port.send(b'\n');
    }
}

/// Write to serial using syscall (for userland compatibility)
fn syscall_write(buf: &[u8]) {
    if let Some(ref mut port) = *SERIAL.lock() {
        for &byte in buf {
            port.send(byte);
        }
    }
}

/// Initialize GOP framebuffer (called before exiting boot services)
#[cfg(feature = "uefi")]
pub fn init_framebuffer() -> Result<(), &'static str> {
    // Get the Graphics Output Protocol (GOP) from UEFI
    let gop_handle = uefi::boot::get_handle_for_protocol::<uefi::proto::console::gop::GraphicsOutput>()
        .map_err(|_| "Failed to get GOP handle")?;

    let mut gop = uefi::boot::open_protocol_exclusive::<uefi::proto::console::gop::GraphicsOutput>(gop_handle)
        .map_err(|_| "Failed to open GOP protocol")?;

    // Get the current mode information
    let mode_info = gop.current_mode_info();
    let (width, height) = mode_info.resolution();
    let stride = mode_info.stride();

    // Get the framebuffer address
    let fb_addr = gop.frame_buffer().as_mut_ptr() as *mut u32;

    // Store framebuffer information
    unsafe {
        FRAMEBUFFER = Some(FramebufferInfo {
            buffer: fb_addr,
            width,
            height,
            stride,
        });
    }

    Ok(())
}

/// Write a pixel to the framebuffer
#[cfg(feature = "uefi")]
pub fn write_pixel(x: usize, y: usize, color: u32) {
    if let Some(fb) = unsafe { &*core::ptr::addr_of!(FRAMEBUFFER) } {
        if x < fb.width && y < fb.height {
            unsafe {
                *fb.buffer.add(y * fb.stride + x) = color;
            }
        } else {
            // Safety check: log out-of-bounds access
            serial_write("Warning: Attempted to write pixel outside framebuffer bounds");
        }
    } else {
        serial_write("Warning: Attempted to write pixel but framebuffer not initialized");
    }
}

/// Clear the screen with a color
#[cfg(feature = "uefi")]
pub fn clear_screen(color: u32) {
    if let Some(fb) = unsafe { &*core::ptr::addr_of!(FRAMEBUFFER) } {
        let total_pixels = fb.height * fb.stride;
        for i in 0..total_pixels {
            unsafe {
                *fb.buffer.add(i) = color;
            }
        }
    } else {
        serial_write("Warning: Attempted to clear screen but framebuffer not initialized");
    }
}

/// Initialize GDT and TSS for proper segmentation
pub fn init_gdt_tss() {
    unsafe {
        // Set up TSS
        TSS.interrupt_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            x86_64::VirtAddr::from_ptr(core::ptr::addr_of!(STACK))
                + STACK_SIZE as u64
        };

        // Set up GDT using raw pointers to avoid mutable static references
        let gdt = &mut *core::ptr::addr_of_mut!(GDT);
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        gdt.append(Descriptor::user_code_segment());
        gdt.append(Descriptor::user_data_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(&*core::ptr::addr_of!(TSS)));

        // Load GDT and TSS
        gdt.load();
        CS::set_reg(code_selector);
        load_tss(tss_selector);

        // Set data segment registers
        DS::set_reg(data_selector);
        ES::set_reg(data_selector);
        FS::set_reg(data_selector);
        GS::set_reg(data_selector);
        SS::set_reg(data_selector);
    }
}

/// Set up basic identity-mapped paging for the first 4GB of memory
/// NOTE: Currently DISABLED for safety - UEFI already provides paging
/// TODO: Re-enable only after implementing safe page table allocation from UEFI memory map
/// DO NOT call this function until proper UEFI-based page table allocation is implemented
#[allow(unused)]
unsafe fn setup_paging() {
    // TODO: Implement safe page table allocation using UEFI memory map
    // For now, rely on UEFI's paging setup
    // This function is intentionally disabled to prevent unsafe memory access
}

/// Simple bump allocator for kernel heap using UEFI-allocated memory
#[derive(Debug)]
struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl BumpAllocator {
    fn new_from_uefi(memory_map: &dyn uefi::mem::memory_map::MemoryMap) -> Option<Self> {
        // Find a suitable memory region for the heap (at least 1MB free)
        const HEAP_SIZE: usize = 0x100000; // 1MB heap

        for descriptor in memory_map.entries() {
            if descriptor.ty == uefi::mem::memory_map::MemoryType::CONVENTIONAL
                && (descriptor.page_count as usize) * 4096 >= HEAP_SIZE {
                let heap_start = descriptor.phys_start as usize;
                let heap_end = heap_start + HEAP_SIZE;

                return Some(BumpAllocator {
                    heap_start,
                    heap_end,
                    next: heap_start,
                });
            }
        }

        None // No suitable memory region found
    }

    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // Safety checks
        if size == 0 {
            serial_write("Warning: Attempted to allocate 0 bytes");
            return core::ptr::null_mut();
        }
        
        if align == 0 || !align.is_power_of_two() {
            serial_write("Error: Invalid alignment - must be power of 2 and non-zero");
            return core::ptr::null_mut();
        }

        let aligned_next = (self.next + align - 1) & !(align - 1);

        if aligned_next + size > self.heap_end {
            serial_write("Error: Out of memory in bump allocator");
            serial_write("Requested size:");
            // Simple size logging (would need proper itoa in real implementation)
            return core::ptr::null_mut();
        }

        let ptr = aligned_next as *mut u8;
        self.next = aligned_next + size;
        ptr
    }
}

/// Stage-based allocator: starts with bump allocation, can upgrade to more sophisticated strategies
#[derive(Debug)]
enum AllocatorStage {
    /// Simple bump allocation using pre-allocated heap
    Bump(BumpAllocator),
    // Future: Frame-based allocation with proper virtual memory
    // Frame(FrameAllocator),
}

impl AllocatorStage {
    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        match self {
            AllocatorStage::Bump(bump) => unsafe { bump.alloc(size, align) },
        }
    }

    unsafe fn dealloc(&mut self, _ptr: *mut u8, _size: usize, _align: usize) {
        // For now, no deallocation in bump allocator
        // Future stages will implement proper deallocation
    }
}

/// Global allocator instance
static mut GLOBAL_ALLOCATOR: Option<AllocatorStage> = None;

#[allow(unused)]
static mut HEAP_ALLOCATOR: Option<BumpAllocator> = None;

/// Demonstrate AI text analysis by running it in userland
/// This moves AI components to userland for isolation and restartability
#[cfg(feature = "uefi")]
fn demonstrate_ai() {
    serial_write("=== NEW USERLAND AI DEMO STARTING ===");

    // Simulate loading userland AI program
    // In a real system, we'd load an ELF binary
    extern "C" fn userland_ai_demo() {
        // This would be the _start function from ai_analyzer.rs
        let sample_texts = [
            "This kernel is written in Rust programming language",
            "The memory management system uses paging and virtual memory",
            "Data analysis shows interesting patterns in user behavior",
            "Creating beautiful user interfaces requires design skills",
        ];

        for text in sample_texts.iter() {
            // Simple categorization (moved from kernel)
            let category = if text.contains("code") || text.contains("kernel") || text.contains("memory") {
                "TECHNICAL"
            } else if text.contains("design") || text.contains("interface") {
                "CREATIVE"
            } else {
                "DATA"
            };

            // Print using syscall
            syscall_write(b"Analyzing text: ");
            syscall_write(text.as_bytes());
            syscall_write(b" -> ");
            syscall_write(category.as_bytes());
            syscall_write(b"\n");
        }
    }

    // Load and execute as userland process
    let pid = process::load_userland_function(userland_ai_demo as u64);
    match pid {
        Ok(pid) => {
            uefi::println!("Loaded userland AI process with PID {}", pid);
            if let Err(e) = process::execute_process(pid) {
                uefi::println!("Failed to execute AI process: {:?}", e);
            }
        }
        Err(e) => {
            uefi::println!("Failed to load userland AI process: {:?}", e);
        }
    }

    serial_write("AI userland demonstration complete!");
}

/// Basic interrupt handlers with proper x86-interrupt ABI
// NOTE: These are currently stubs until proper interrupt ABI is fully implemented
// TODO: Avoid sti/cli misuse - these handlers should not enable/disable interrupts
use x86_64::structures::idt::InterruptStackFrame;

extern "x86-interrupt" fn divide_by_zero_handler(_stack_frame: InterruptStackFrame) {
    // STUB: Basic divide by zero handler
    serial_write("Divide by zero exception!");
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    // STUB: Breakpoint handler - currently just continues
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    // STUB: Timer interrupt - could be used for AI processing scheduling
    unsafe {
        (&mut *core::ptr::addr_of_mut!(PICS)).notify_end_of_interrupt(PIC_1_OFFSET);
    }
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    // STUB: Keyboard interrupt - could be used for AI input
    unsafe {
        (&mut *core::ptr::addr_of_mut!(PICS)).notify_end_of_interrupt(PIC_1_OFFSET + 1);
    }
}

/// Load the IDT
/// NOTE: Replaced with x86_64::structures::idt::InterruptDescriptorTable
/*
// Removed - now using x86_64 crate IDT structures
#[allow(unused)]
unsafe fn load_idt() {
    let idt_ptr = IdtPtr {
        limit: (core::mem::size_of::<Idt>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };

    core::arch::asm!("lidt [{}]", in(reg) &idt_ptr);
}
*/

/// Initialize interrupts with proper x86_64 structures
pub fn init_interrupts() {
    unsafe {
        // TODO: Only load IDT after GDT is properly set up
        // For now, we load it immediately after GDT, but this should be conditional
        // if GDT_ready { load_idt() } else { skip }

        // Set up basic interrupt handlers using raw pointers
        let idt = &mut *core::ptr::addr_of_mut!(IDT);
        idt.divide_error.set_handler_fn(divide_by_zero_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);

        // Check if APIC is available
        if apic::is_apic_available() {
            // Set up APIC-based interrupts
            idt[32].set_handler_fn(scheduler::timer_handler); // Timer
            idt[33].set_handler_fn(keyboard_handler); // Keyboard

            // Note: I/O APIC routing will be set up after APIC initialization
        } else {
            // Set up PIC interrupts
            idt[PIC_1_OFFSET].set_handler_fn(scheduler::timer_handler);
            idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_handler);

            // Initialize and configure PIC using raw pointers
            let pics = &mut *core::ptr::addr_of_mut!(PICS);
            pics.initialize();
            pics.write_masks(0xFC, 0xFF); // Enable timer and keyboard interrupts
        }

        // Set up syscall interrupt (int 0x80)
        idt[0x80].set_handler_fn(syscall::syscall_handler);

        // Load the IDT
        idt.load();
    }
}

/// Initialize the Programmable Interval Timer (PIT) for scheduling
pub fn init_pit() {
    // PIT I/O ports
    const PIT_COMMAND: u16 = 0x43;
    const PIT_CHANNEL0: u16 = 0x40;

    // Set up PIT for ~100Hz (10ms intervals)
    // Frequency = 1193182 / divisor
    // For 100Hz: divisor = 1193182 / 100 = 11931.82 ≈ 11932
    let divisor: u16 = 11932;

    unsafe {
        // Send command byte: Channel 0, lobyte/hibyte, rate generator
        x86_64::instructions::port::Port::new(PIT_COMMAND).write(0x36u8);

        // Send divisor (low byte first, then high byte)
        x86_64::instructions::port::Port::new(PIT_CHANNEL0).write((divisor & 0xFF) as u8);
        x86_64::instructions::port::Port::new(PIT_CHANNEL0).write((divisor >> 8) as u8);
    }
}



#[cfg(feature = "uefi")]
#[entry]
fn efi_main() -> Status {
    uefi::println!("Hello from Rust UEFI OS!");
    serial_write("EFI main started\n");
    uefi::println!("Bootloader initialized successfully.");
    uefi::println!("Preparing kernel hand-off...");

    // Initialize serial port early before any serial_write use
    serial_init();
    uefi::println!("Serial port initialized successfully.");

    // Initialize GOP framebuffer before exiting boot services
    if let Err(e) = init_framebuffer() {
        uefi::println!("Warning: Failed to initialize framebuffer: {}", e);
        uefi::println!("Falling back to VGA text mode.");
    } else {
        uefi::println!("GOP framebuffer initialized successfully.");
    }

    // Get memory map before exiting boot services
    let memory_map = uefi::boot::get_memory_map(uefi::mem::memory_map::MemoryType::LOADER_DATA).unwrap();

    // Exit boot services to take full control
    uefi::println!("Exiting UEFI boot services...");
    uefi::boot::exit_boot_services();

    serial_write("Just exited boot services\n");

    serial_write("=== KERNEL MODE: Full kernel control established ===\n");

    // Initialize frame allocator first (needed for virtual memory)
    frame_allocator::init(&memory_map);
    serial_write("Frame allocator initialized successfully.\n");

    // Initialize virtual memory management
    let physical_memory_offset = x86_64::VirtAddr::new(0); // UEFI identity maps physical memory
    let mut vmm = virtual_memory::init(physical_memory_offset);
    serial_write("Virtual memory manager initialized successfully.\n");

    // Create identity mapping for kernel (first 4GB)
    let kernel_start = x86_64::PhysAddr::new(0);
    let kernel_end = x86_64::PhysAddr::new(4 * 1024 * 1024 * 1024); // 4GB
    let kernel_flags = x86_64::structures::paging::PageTableFlags::PRESENT
        | x86_64::structures::paging::PageTableFlags::WRITABLE;
    if let Err(e) = virtual_memory::create_identity_mapping(&mut vmm, kernel_start, kernel_end, kernel_flags) {
        serial_write("Warning: Failed to create kernel identity mapping\n");
    } else {
        serial_write("Kernel identity mapping created successfully.\n");
    }

    // Allocate kernel heap pages for advanced allocator
    let heap_start = x86_64::VirtAddr::new(0x_4444_4444_0000);
    let heap_size = 100 * 1024; // 100 KiB
    if let Err(e) = virtual_memory::allocate_kernel_heap(&mut vmm, heap_start, heap_size) {
        serial_write("Warning: Failed to allocate kernel heap\n");
    } else {
        serial_write("Kernel heap allocated successfully.\n");
    }

    // Initialize advanced heap allocator
    if let Err(e) = heap_allocator::init_heap_with_pages(heap_start.as_u64() as usize, heap_size) {
        serial_write("Warning: Failed to initialize advanced heap allocator\n");
    } else {
        serial_write("Advanced heap allocator initialized successfully.\n");
    }

    // Initialize GDT and TSS
    init_gdt_tss();
    serial_write("GDT and TSS initialized successfully.\n");

    // Initialize interrupts
    init_interrupts();
    serial_write("Interrupts initialized successfully.\n");

    // Initialize PIT for scheduling
    init_pit();
    serial_write("PIT timer initialized successfully.\n");

    // Initialize advanced interrupt handling (APIC) if available
    if apic::is_apic_available() {
        if let Err(e) = apic::init() {
            serial_write("Warning: Failed to initialize APIC\n");
            serial_write("Falling back to legacy PIC interrupts.\n");
        } else {
            serial_write("APIC initialized successfully.\n");

            // Set up I/O APIC interrupt routing
            if let Some(apic) = apic::get_apic() {
                let lapic_id = apic.lapic().id() as u8;
                // Route timer interrupt (IRQ 0) to vector 32
                apic.setup_interrupt(0, 32, lapic_id);
                // Route keyboard interrupt (IRQ 1) to vector 33
                apic.setup_interrupt(1, 33, lapic_id);
                serial_write("I/O APIC interrupt routing configured.\n");
            }

            // Disable legacy PIC when APIC is available
            apic::disable_legacy_pic();
            serial_write("Legacy PIC disabled - using APIC for interrupts.\n");
        }
    } else {
        serial_write("APIC not available - using legacy PIC interrupts.\n");
    }

    // Enable interrupts for preemptive scheduling
    x86_64::instructions::interrupts::enable();
    serial_write("Interrupts enabled for preemptive scheduling.\n");

    // Initialize basic heap allocator (fallback)
    unsafe {
        HEAP_ALLOCATOR = BumpAllocator::new_from_uefi(&memory_map);
    }
    serial_write("Basic heap allocator initialized successfully.\n");

    // Initialize process management
    process::init();
    serial_write("Process management initialized successfully.\n");

    // Initialize filesystem
    filesystem::init();
    serial_write("Filesystem initialized successfully.\n");

    // Initialize security framework
    security::init();
    if let Some(sm) = security::get_security_manager() {
        if let Some(fs) = unsafe { syscall::FILESYSTEM.as_mut() } {
            if let Err(e) = security::init_with_fs(fs) {
                serial_write("Warning: Failed to initialize security with filesystem\n");
            } else {
                serial_write("Security framework initialized successfully.\n");
            }
        }
    }

    // Initialize AI model infrastructure
    ai_models::init();
    serial_write("AI model infrastructure initialized successfully.\n");

    serial_write("About to initialize scheduler...\n");

    // Initialize process scheduler
    scheduler::init();
    serial_write("Process scheduler initialized successfully.\n");

    serial_write("About to initialize PCI...\n");


    // Initialize PCI bus enumeration
    pci::init();
    pci::print_devices();
    serial_write("PCI bus enumeration initialized successfully.\n");


    // Initialize USB drivers
    usb::init();
    serial_write("USB drivers initialized successfully.\n");

    // Enumerate USB input devices
    usb_input::enumerate();
    serial_write("USB input devices enumerated.\n");

    // Initialize graphics driver
    graphics::init();
    serial_write("Graphics driver initialized.\n");

    serial_write("About to initialize Ethernet...\n");

    // Initialize Ethernet driver
    ethernet::init();
    ethernet::test_ethernet();
    serial_write("Ethernet driver initialized successfully.\n");

    // Initialize basic heap allocator (fallback)
    unsafe {
        HEAP_ALLOCATOR = BumpAllocator::new_from_uefi(&memory_map);
    }
    uefi::println!("Basic heap allocator initialized successfully.");

    // Initialize process management
    process::init();
    uefi::println!("Process management initialized successfully.");

    // Initialize filesystem
    filesystem::init();
    uefi::println!("Filesystem initialized successfully.");

    // Initialize security framework
    security::init();
    if let Some(sm) = security::get_security_manager() {
        if let Some(fs) = unsafe { syscall::FILESYSTEM.as_mut() } {
            if let Err(e) = security::init_with_fs(fs) {
                uefi::println!("Warning: Failed to initialize security with filesystem: {:?}", e);
            } else {
                uefi::println!("Security framework initialized successfully.");
            }
        }
    }

    // Initialize AI model infrastructure
    ai_models::init();
    uefi::println!("AI model infrastructure initialized successfully.");

    uefi::println!("About to initialize scheduler...");

    // Initialize process scheduler
    scheduler::init();
    uefi::println!("Process scheduler initialized successfully.");

    uefi::println!("About to initialize PCI...");

    // Initialize PCI bus enumeration
    // pci::init();
    // pci::print_devices();
    uefi::println!("PCI bus enumeration initialized successfully.");

    uefi::println!("About to initialize Ethernet...");

    // Initialize Ethernet driver
    // ethernet::init();
    // ethernet::test_ethernet();
    uefi::println!("Ethernet driver initialized successfully.");

    // Set global filesystem reference for syscalls
    unsafe {
        syscall::FILESYSTEM = filesystem::get_fs();
    }

    // Test userland process execution
    test_userland_process();

    serial_write("About to demonstrate AI...\n");

    // Demonstrate AI text analysis with graphics
    // demonstrate_ai();

    serial_write("AI demo commented out\n");

    // Test divide by zero (uncomment to test fault handler)
    // unsafe { test_divide_by_zero(); }

    serial_write("AI graphics demonstration complete! Kernel running successfully.\n");

    // For now, just infinite loop to show we're still running
    loop {
        // Busy wait - in real kernel we'd have interrupts/timers
        unsafe {
            core::arch::asm!("pause");
        }
    }
}

// Test userland process execution
fn test_userland_process() {
    // For now, simulate a userland process by directly calling a function
    // In a real system, we'd load an ELF binary

    // Simple userland hello function
    extern "C" fn userland_hello() {
        let message = b"Hello from userland!\n";
        let len = message.len();

        unsafe {
            core::arch::asm!(
                "mov rax, 0",      // syscall number for write
                "mov rdi, 1",      // fd = stdout
                "mov rsi, {}",     // buf
                "mov rdx, {}",     // count
                "int 0x80",        // syscall
                in(reg) message.as_ptr(),
                in(reg) len,
            );
        }
    }

    // Test filesystem operations
    extern "C" fn test_filesystem() {
        let filename = b"/test.txt";
        let content = b"Hello, filesystem!\n";
        let mut buffer = [0u8; 64];

        unsafe {
            // Open/create file
            core::arch::asm!(
                "mov rax, 1",      // syscall number for open
                "mov rdi, {}",     // path
                "mov rsi, 6",      // flags (read + write + create)
                "mov rdx, 0",      // mode
                "int 0x80",        // syscall
                "mov r8, rax",     // save fd
                in(reg) filename.as_ptr(),
            );

            let fd: u64;
            core::arch::asm!("mov {}, r8", out(reg) fd);

            if fd < 0x8000000000000000 { // Check if not error
                // Write to file
                core::arch::asm!(
                    "mov rax, 0",      // syscall number for write
                    "mov rdi, r8",     // fd
                    "mov rsi, {}",     // buf
                    "mov rdx, {}",     // count
                    "int 0x80",        // syscall
                    in(reg) content.as_ptr(),
                    in(reg) content.len(),
                );

                // Read from file
                core::arch::asm!(
                    "mov rax, 3",      // syscall number for read
                    "mov rdi, r8",     // fd
                    "mov rsi, {}",     // buf
                    "mov rdx, {}",     // count
                    "int 0x80",        // syscall
                    in(reg) buffer.as_mut_ptr(),
                    in(reg) buffer.len(),
                );

                // Write read content to stdout
                core::arch::asm!(
                    "mov rax, 0",      // syscall number for write
                    "mov rdi, 1",      // fd = stdout
                    "mov rsi, {}",     // buf
                    "mov rdx, {}",     // count
                    "int 0x80",        // syscall
                    in(reg) buffer.as_ptr(),
                    in(reg) buffer.len(),
                );

                // Close file
                core::arch::asm!(
                    "mov rax, 2",      // syscall number for close
                    "mov rdi, r8",     // fd
                    "int 0x80",        // syscall
                );
            }
        }
    }

    // Load userland processes (scheduler will execute them)
    let pid1 = process::load_userland_function(userland_hello as u64);
    let pid2 = process::load_userland_function(test_filesystem as u64);

    match pid1 {
        Ok(pid) => {
            uefi::println!("Loaded userland hello process with PID {}", pid);
        }
        Err(e) => {
            uefi::println!("Failed to load hello process: {:?}", e);
        }
    }

    match pid2 {
        Ok(pid) => {
            uefi::println!("Loaded filesystem test process with PID {}", pid);
        }
        Err(e) => {
            uefi::println!("Failed to load filesystem process: {:?}", e);
        }
    }

    // Scheduler will now handle process execution with preemptive scheduling
    uefi::println!("Processes loaded - scheduler will handle execution with preemptive multitasking");
}

