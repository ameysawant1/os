#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, panic_info_message)]

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

// Enhanced panic handler with detailed error information
#[cfg(feature = "uefi")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Try to get serial output first
    if let Some(ref mut port) = unsafe { SERIAL.lock().as_mut() } {
        port.send(b'\n');
        port.send(b'P');
        port.send(b'A');
        port.send(b'N');
        port.send(b'I');
        port.send(b'C');
        port.send(b':');
        port.send(b' ');
        
        // Print panic message if available
        if let Some(message) = info.message() {
            for byte in message.as_str().unwrap_or("Unknown panic").bytes() {
                port.send(byte);
            }
        }
        port.send(b'\n');
        
        // Print location information
        if let Some(location) = info.location() {
            let file = location.file();
            let line = location.line();
            
            port.send(b'F');
            port.send(b'i');
            port.send(b'l');
            port.send(b'e');
            port.send(b':');
            port.send(b' ');
            for byte in file.bytes() {
                port.send(byte);
            }
            port.send(b':');
            
            // Convert line number to string
            let line_str = line.to_string();
            for byte in line_str.bytes() {
                port.send(byte);
            }
            port.send(b'\n');
        }
    }
    
    // Also try UEFI console output
    if let Some(message) = info.message() {
        uefi::println!("PANIC: {}", message);
    } else {
        uefi::println!("PANIC: Unknown panic occurred");
    }
    
    if let Some(location) = info.location() {
        uefi::println!("Location: {}:{}", location.file(), location.line());
    }
    
    // Print stack trace information (basic)
    uefi::println!("Kernel panic - halting system");
    
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

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

/// Write a pixel to the framebuffer
pub fn write_pixel(x: usize, y: usize, color: u32) {
    if let Some(fb) = unsafe { &*core::ptr::addr_of!(FRAMEBUFFER) } {
        if x < fb.width && y < fb.height {
            unsafe {
                *fb.buffer.add(y * fb.stride + x) = color;
            }
        }
    }
}

/// Clear the screen with a color
pub fn clear_screen(color: u32) {
    if let Some(fb) = unsafe { &*core::ptr::addr_of!(FRAMEBUFFER) } {
        for y in 0..fb.height {
            for x in 0..fb.width {
                unsafe {
                    *fb.buffer.add(y * fb.stride + x) = color;
                }
            }
        }
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
/// NOTE: Currently disabled for safety - UEFI already provides paging
#[allow(unused)]
unsafe fn setup_paging() {
    // TODO: Implement safe page table allocation using UEFI memory map
    // For now, rely on UEFI's paging setup
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

/// Basic AI Text Analyzer for semantic processing
struct TextAnalyzer {
    // Simple keyword-based categorization
    tech_keywords: &'static [&'static str],
    creative_keywords: &'static [&'static str],
    data_keywords: &'static [&'static str],
}

impl TextAnalyzer {
    const fn new() -> Self {
        TextAnalyzer {
            tech_keywords: &["code", "program", "kernel", "memory", "cpu", "system", "os", "rust", "compile", "algorithm", "software", "hardware", "computer", "programming", "development"],
            creative_keywords: &["design", "art", "music", "write", "writing", "create", "story", "stories", "image", "video", "creative", "aesthetic", "beautiful", "interface", "ui", "ux"],
            data_keywords: &["data", "analyze", "chart", "graph", "statistics", "database", "query", "search", "analytics", "visualization", "pattern", "trend", "model"],
        }
    }

    fn analyze_text(&self, text: &str) -> TextCategory {
        // Safety check: limit text length to prevent buffer overflows
        let text_bytes = text.as_bytes();
        if text_bytes.len() > 255 {
            serial_write("Warning: Text too long for analysis, truncating");
        }
        
        let mut text_lower = [0u8; 256];
        for (i, &byte) in text_bytes.iter().enumerate().take(255) {
            text_lower[i] = byte.to_ascii_lowercase();
        }

        let mut tech_score = 0;
        let mut creative_score = 0;
        let mut data_score = 0;

        // Count keyword matches (case-insensitive)
        for &keyword in self.tech_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                tech_score += 1;
            }
        }

        for &keyword in self.creative_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                creative_score += 1;
            }
        }

        for &keyword in self.data_keywords.iter() {
            if self.contains_keyword(&text_lower, keyword) {
                data_score += 1;
            }
        }

        // Determine category based on highest score
        if tech_score >= creative_score && tech_score >= data_score && tech_score > 0 {
            TextCategory::Technical
        } else if creative_score >= data_score && creative_score > 0 {
            TextCategory::Creative
        } else if data_score > 0 {
            TextCategory::Data
        } else {
            // No keywords matched - default to Data
            TextCategory::Data
        }
    }

    fn contains_keyword(&self, text_lower: &[u8; 256], keyword: &str) -> bool {
        let keyword_bytes = keyword.as_bytes();
        let keyword_len = keyword_bytes.len();
        
        if keyword_len == 0 {
            return false;
        }
        
        // Safety check: ensure keyword length is reasonable
        if keyword_len > 256 {
            return false;
        }

        let text_len = text_lower.iter().position(|&x| x == 0).unwrap_or(text_lower.len());

        for i in 0..=(text_len.saturating_sub(keyword_len)) {
            let mut matches = true;
            for j in 0..keyword_len {
                let keyword_char = keyword_bytes[j].to_ascii_lowercase();
                if text_lower[i + j] != keyword_char {
                    matches = false;
                    break;
                }
            }
            if matches {
                return true;
            }
        }
        false
    }

    fn extract_features(&self, text: &str) -> TextFeatures {
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();
        let has_numbers = text.chars().any(|c| c.is_numeric());
        let has_punctuation = text.chars().any(|c| !c.is_alphanumeric() && !c.is_whitespace());

        TextFeatures {
            char_count,
            word_count,
            has_numbers,
            has_punctuation,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TextCategory {
    Technical,
    Creative,
    Data,
}

#[derive(Debug, Clone, Copy)]
struct TextFeatures {
    #[allow(unused)]
    char_count: usize,
    #[allow(unused)]
    word_count: usize,
    #[allow(unused)]
    has_numbers: bool,
    #[allow(unused)]
    has_punctuation: bool,
}

static mut TEXT_ANALYZER: TextAnalyzer = TextAnalyzer::new();

/// Demonstrate AI text analysis with graphics
#[cfg(feature = "uefi")]
unsafe fn demonstrate_ai() {
    serial_write("Starting AI text analysis demonstration with graphics...");

    // Clear screen with dark background
    clear_screen(0x00112233); // Dark blue background

    let sample_texts = [
        "This kernel is written in Rust programming language",
        "The memory management system uses paging and virtual memory",
        "Data analysis shows interesting patterns in user behavior",
        "Creating beautiful user interfaces requires design skills",
    ];

    // Analyze each text sample and display graphically
    for (i, &text) in sample_texts.iter().enumerate() {
        let category = unsafe { (*core::ptr::addr_of!(TEXT_ANALYZER)).analyze_text(text) };
        let _features = unsafe { (*core::ptr::addr_of!(TEXT_ANALYZER)).extract_features(text) };

        // Calculate position for this sample
        let y_offset = 100 + i * 120;
        let x_start = 50;

        // Draw colored rectangle based on category
        let (color, category_name) = match category {
            TextCategory::Technical => (0x00FF4444, "TECHNICAL"), // Red
            TextCategory::Creative => (0x004444FF, "CREATIVE"),   // Blue
            TextCategory::Data => (0x0044FF44, "DATA"),          // Green
        };

        // Draw category rectangle
        for y in y_offset..(y_offset + 80) {
            for x in x_start..(x_start + 200) {
                write_pixel(x, y, color);
            }
        }

        // Draw white border
        for y in y_offset..(y_offset + 80) {
            write_pixel(x_start, y, 0x00FFFFFF);
            write_pixel(x_start + 199, y, 0x00FFFFFF);
        }
        for x in x_start..(x_start + 200) {
            write_pixel(x, y_offset, 0x00FFFFFF);
            write_pixel(x, y_offset + 79, 0x00FFFFFF);
        }

        // Log to serial
        serial_write("Analyzing text sample...");
        serial_write(category_name);
        serial_write(text);
    }

    // Draw title at the top
    let title = "AI Text Analyzer - Graphics Mode";
    let title_y = 20;
    let title_x = 50;

    // Draw title background
    for y in title_y..(title_y + 30) {
        for x in title_x..(title_x + 400) {
            write_pixel(x, y, 0x00888888); // Gray background
        }
    }

    // Draw title text (simple white rectangles for letters - very basic)
    // This is a very simple text rendering - in a real system you'd use a font
    for (i, _) in title.chars().enumerate() {
        let char_x = title_x + 10 + i * 12;
        for y in (title_y + 5)..(title_y + 25) {
            for x in char_x..(char_x + 8) {
                write_pixel(x, y, 0x00FFFFFF);
            }
        }
    }

    // Show AI status at bottom
    let status_y = 600;
    let status_x = 50;

    for y in status_y..(status_y + 20) {
        for x in status_x..(status_x + 300) {
            write_pixel(x, y, 0x00880088); // Purple background
        }
    }

    serial_write("AI graphics demonstration complete!");
}

/// Basic interrupt handlers with proper x86-interrupt ABI
use x86_64::structures::idt::InterruptStackFrame;

extern "x86-interrupt" fn divide_by_zero_handler(_stack_frame: InterruptStackFrame) {
    // Handler logic - for now just print and halt
    serial_write("Divide by zero exception!");
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    // For now, just continue on breakpoint
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    // Timer interrupt - could be used for AI processing scheduling
    unsafe {
        (&mut *core::ptr::addr_of_mut!(PICS)).notify_end_of_interrupt(PIC_1_OFFSET);
    }
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    // Keyboard interrupt - could be used for AI input
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
        // Set up basic interrupt handlers using raw pointers
        let idt = &mut *core::ptr::addr_of_mut!(IDT);
        idt.divide_error.set_handler_fn(divide_by_zero_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);

        // Set up PIC interrupts
        idt[PIC_1_OFFSET].set_handler_fn(timer_handler);
        idt[PIC_1_OFFSET + 1].set_handler_fn(keyboard_handler);

        // Load the IDT
        idt.load();

        // Initialize and configure PIC using raw pointers
        let pics = &mut *core::ptr::addr_of_mut!(PICS);
        pics.initialize();
        pics.write_masks(0xFC, 0xFF); // Enable timer and keyboard interrupts
    }
}

#[cfg(feature = "uefi")]
unsafe fn test_divide_by_zero() {
    let _ = 1 / 0;
}

#[cfg(feature = "uefi")]
#[entry]
fn efi_main() -> Status {
    uefi::println!("Hello from Rust UEFI OS!");
    uefi::println!("Bootloader initialized successfully.");
    uefi::println!("Preparing kernel hand-off...");

    // Initialize GOP framebuffer before exiting boot services
    if let Err(e) = init_framebuffer() {
        uefi::println!("Warning: Failed to initialize framebuffer: {}", e);
        uefi::println!("Falling back to VGA text mode.");
    } else {
        uefi::println!("GOP framebuffer initialized successfully.");
    }

    // Exit boot services to take full control
    uefi::println!("Exiting UEFI boot services...");
    let memory_map = unsafe { uefi::boot::exit_boot_services(Some(uefi::mem::memory_map::MemoryType::LOADER_DATA)) };

    uefi::println!("=== KERNEL MODE: Full kernel control established ===");

    // Initialize GDT and TSS
    init_gdt_tss();
    uefi::println!("GDT and TSS initialized successfully.");

    // Initialize interrupts
    init_interrupts();
    uefi::println!("Interrupts initialized successfully.");

    // Initialize heap allocator from UEFI memory map
    unsafe {
        HEAP_ALLOCATOR = BumpAllocator::new_from_uefi(&memory_map);
    }
    uefi::println!("Heap allocator initialized successfully.");

    // Initialize serial port
    serial_init();
    uefi::println!("Serial port initialized successfully.");

    // Demonstrate AI text analysis with graphics
    unsafe {
        demonstrate_ai();
    }

    // Test divide by zero (uncomment to test fault handler)
    // unsafe { test_divide_by_zero(); }

    uefi::println!("AI graphics demonstration complete! Kernel running successfully.");

    // For now, just infinite loop to show we're still running
    loop {
        // Busy wait - in real kernel we'd have interrupts/timers
        unsafe {
            core::arch::asm!("pause");
        }
    }
}

// Test divide by zero
unsafe fn test_divide_by_zero() {
    let _ = 1 / 0;
}

