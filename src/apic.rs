//! Advanced Interrupt Handling with APIC
//!
//! Implements APIC (Advanced Programmable Interrupt Controller) support for:
//! - Local APIC (LAPIC) for per-CPU interrupts
//! - I/O APIC for external device interrupts
//! - MSI/MSI-X support for modern PCI devices
//! - SMP (Symmetric Multi-Processing) support

use x86_64::instructions::port::Port;
use core::ptr;

/// APIC register offsets (relative to APIC base)
const LAPIC_ID: usize = 0x20;
const LAPIC_VERSION: usize = 0x30;
const LAPIC_TPR: usize = 0x80;        // Task Priority Register
const LAPIC_APR: usize = 0x90;        // Arbitration Priority Register
const LAPIC_PPR: usize = 0xA0;        // Processor Priority Register
const LAPIC_EOI: usize = 0xB0;        // End of Interrupt
const LAPIC_RRD: usize = 0xC0;        // Remote Read Register
const LAPIC_LDR: usize = 0xD0;        // Logical Destination Register
const LAPIC_DFR: usize = 0xE0;        // Destination Format Register
const LAPIC_SVR: usize = 0xF0;        // Spurious Interrupt Vector Register
const LAPIC_ISR: usize = 0x100;       // In-Service Register (256 bits)
const LAPIC_TMR: usize = 0x180;       // Trigger Mode Register (256 bits)
const LAPIC_IRR: usize = 0x200;       // Interrupt Request Register (256 bits)
const LAPIC_ESR: usize = 0x280;       // Error Status Register
const LAPIC_ICR_LOW: usize = 0x300;   // Interrupt Command Register (low)
const LAPIC_ICR_HIGH: usize = 0x310;  // Interrupt Command Register (high)
const LAPIC_LVT_TIMER: usize = 0x320; // Local Vector Table - Timer
const LAPIC_LVT_THERMAL: usize = 0x330; // Local Vector Table - Thermal
const LAPIC_LVT_PERF: usize = 0x340;   // Local Vector Table - Performance
const LAPIC_LVT_LINT0: usize = 0x350;  // Local Vector Table - LINT0
const LAPIC_LVT_LINT1: usize = 0x360;  // Local Vector Table - LINT1
const LAPIC_LVT_ERROR: usize = 0x370;  // Local Vector Table - Error
const LAPIC_TIMER_ICR: usize = 0x380;  // Timer Initial Count Register
const LAPIC_TIMER_CCR: usize = 0x390;  // Timer Current Count Register
const LAPIC_TIMER_DCR: usize = 0x3E0;  // Timer Divide Configuration Register

/// I/O APIC register offsets
const IOAPIC_IOAPICID: usize = 0x00;
const IOAPIC_IOAPICVER: usize = 0x01;
const IOAPIC_IOAPICARB: usize = 0x02;
const IOAPIC_IOREDTBL: usize = 0x10;   // Start of I/O Redirection Table

/// APIC base MSR
const IA32_APIC_BASE: u32 = 0x1B;

/// Local APIC structure
pub struct LocalApic {
    base_addr: u64,
}

impl LocalApic {
    /// Create a new Local APIC instance
    pub unsafe fn new() -> Option<Self> {
        // Read APIC base from MSR
        let apic_base = x86_64::registers::model_specific::Msr::new(IA32_APIC_BASE);
        let base_val = apic_base.read();

        // Check if APIC is enabled
        if (base_val & (1 << 11)) == 0 {
            return None; // APIC not enabled
        }

        let base_addr = base_val & 0xFFFFF000; // Mask out lower 12 bits

        Some(LocalApic { base_addr })
    }

    /// Read from APIC register
    unsafe fn read(&self, offset: usize) -> u32 {
        ptr::read_volatile((self.base_addr + offset as u64) as *const u32)
    }

    /// Write to APIC register
    unsafe fn write(&self, offset: usize, value: u32) {
        ptr::write_volatile((self.base_addr + offset as u64) as *mut u32, value);
    }

    /// Get APIC ID
    pub fn id(&self) -> u32 {
        unsafe { self.read(LAPIC_ID) >> 24 }
    }

    /// Get APIC version
    pub fn version(&self) -> u32 {
        unsafe { self.read(LAPIC_VERSION) & 0xFF }
    }

    /// Send End of Interrupt
    pub fn send_eoi(&self) {
        unsafe { self.write(LAPIC_EOI, 0); }
    }

    /// Enable APIC
    pub fn enable(&self) {
        unsafe {
            let svr = self.read(LAPIC_SVR);
            self.write(LAPIC_SVR, svr | 0x100); // Set bit 8 (APIC enable)
        }
    }

    /// Set up timer
    pub fn setup_timer(&self, vector: u8, divide: u32) {
        unsafe {
            // Set divide configuration
            self.write(LAPIC_TIMER_DCR, divide);

            // Set LVT timer register
            self.write(LAPIC_LVT_TIMER, vector as u32);

            // Set initial count (will be set by PIT/APIC timer)
            self.write(LAPIC_TIMER_ICR, 0xFFFFFFFF);
        }
    }

    /// Read timer current count
    pub fn timer_current_count(&self) -> u32 {
        unsafe { self.read(LAPIC_TIMER_CCR) }
    }

    /// Send IPI (Inter-Processor Interrupt)
    pub fn send_ipi(&self, apic_id: u32, vector: u8) {
        unsafe {
            // Set destination
            self.write(LAPIC_ICR_HIGH, apic_id << 24);

            // Set command (fixed delivery, edge triggered, assert)
            self.write(LAPIC_ICR_LOW, (vector as u32) | (1 << 14));
        }
    }
}

/// I/O APIC structure
pub struct IoApic {
    base_addr: u64,
    id: u8,
    max_redir_entries: u8,
}

impl IoApic {
    /// Create I/O APIC instance
    pub unsafe fn new(base_addr: u64) -> Self {
        let id = Self::read_register(base_addr, IOAPIC_IOAPICID) >> 24;
        let ver = Self::read_register(base_addr, IOAPIC_IOAPICVER);
        let max_entries = ((ver >> 16) & 0xFF) as u8 + 1;

        IoApic {
            base_addr,
            id: id as u8,
            max_redir_entries: max_entries,
        }
    }

    /// Read I/O APIC register
    unsafe fn read_register(base: u64, reg: usize) -> u32 {
        // Write register index to IOREGSEL
        ptr::write_volatile(base as *mut u32, reg as u32);
        // Read from IOREGWIN
        ptr::read_volatile((base + 0x10) as *const u32)
    }

    /// Write I/O APIC register
    unsafe fn write_register(base: u64, reg: usize, value: u32) {
        // Write register index to IOREGSEL
        ptr::write_volatile(base as *mut u32, reg as u32);
        // Write value to IOREGWIN
        ptr::write_volatile((base + 0x10) as *mut u32, value);
    }

    /// Set redirection entry
    pub fn set_redirection(&self, index: u8, vector: u8, apic_id: u8, active_low: bool, level_triggered: bool) {
        if index >= self.max_redir_entries {
            return;
        }

        let mut low = vector as u32;
        let mut high = (apic_id as u32) << 24;

        // Set trigger mode and polarity
        if level_triggered {
            low |= 1 << 15; // Level triggered
        }
        if active_low {
            low |= 1 << 13; // Active low
        }

        unsafe {
            Self::write_register(self.base_addr, IOAPIC_IOREDTBL + (index as usize * 2), low);
            Self::write_register(self.base_addr, IOAPIC_IOREDTBL + (index as usize * 2) + 1, high);
        }
    }

    /// Mask/unmask interrupt
    pub fn set_mask(&self, index: u8, masked: bool) {
        if index >= self.max_redir_entries {
            return;
        }

        unsafe {
            let low = Self::read_register(self.base_addr, IOAPIC_IOREDTBL + (index as usize * 2));
            let new_low = if masked { low | (1 << 16) } else { low & !(1 << 16) };
            Self::write_register(self.base_addr, IOAPIC_IOREDTBL + (index as usize * 2), new_low);
        }
    }

    /// Get maximum redirection entries
    pub fn max_entries(&self) -> u8 {
        self.max_redir_entries
    }
}

/// MSI (Message Signaled Interrupts) support
pub struct MsiCapability {
    pub message_address: u32,
    pub message_data: u16,
}

impl MsiCapability {
    /// Configure MSI for a device
    pub fn configure(&self, vector: u8, processor: u8, edge_triggered: bool, assert: bool) -> (u32, u32) {
        let mut address = 0xFEE00000; // MSI address base
        address |= (processor as u32) << 12; // Destination processor

        let mut data = vector as u32;
        if edge_triggered {
            data |= 0 << 14; // Edge triggered
        } else {
            data |= 1 << 14; // Level triggered
        }
        if assert {
            data |= 0 << 15; // Assert
        } else {
            data |= 1 << 15; // Deassert
        }

        (address, data as u32)
    }
}

/// Advanced Interrupt Controller
pub struct AdvancedPic {
    lapic: LocalApic,
    ioapics: [Option<IoApic>; 16], // Support up to 16 I/O APICs
    ioapic_count: usize,
}

impl AdvancedPic {
    /// Initialize advanced interrupt controller
    pub fn new() -> Option<Self> {
        unsafe {
            let lapic = LocalApic::new()?;

            // Enable LAPIC
            lapic.enable();

            Some(AdvancedPic {
                lapic,
                ioapics: [None; 16],
                ioapic_count: 0,
            })
        }
    }

    /// Add I/O APIC
    pub fn add_ioapic(&mut self, base_addr: u64) {
        if self.ioapic_count < self.ioapics.len() {
            unsafe {
                self.ioapics[self.ioapic_count] = Some(IoApic::new(base_addr));
            }
            self.ioapic_count += 1;
        }
    }

    /// Set up interrupt routing
    pub fn setup_interrupt(&mut self, irq: u8, vector: u8, apic_id: u8) {
        // Find appropriate I/O APIC for this IRQ
        // For now, assume single I/O APIC at index 0
        if let Some(ioapic) = &self.ioapics[0] {
            if irq < ioapic.max_entries() {
                ioapic.set_redirection(irq, vector, apic_id, false, false);
                ioapic.set_mask(irq, false); // Unmask
            }
        }
    }

    /// Send End of Interrupt
    pub fn notify_end_of_interrupt(&self, vector: u8) {
        // For LAPIC interrupts, send EOI
        if vector >= 32 && vector <= 255 {
            self.lapic.send_eoi();
        }
    }

    /// Get LAPIC reference
    pub fn lapic(&self) -> &LocalApic {
        &self.lapic
    }

    /// Get LAPIC mutable reference
    pub fn lapic_mut(&mut self) -> &mut LocalApic {
        &mut self.lapic
    }
}

/// Global advanced PIC instance
static mut ADVANCED_PIC: Option<AdvancedPic> = None;

/// Initialize advanced interrupt handling
pub fn init() -> Result<(), &'static str> {
    unsafe {
        ADVANCED_PIC = Some(AdvancedPic::new().ok_or("Failed to initialize APIC")?);
    }

    // Set up default I/O APIC (typically at 0xFEC00000)
    if let Some(apic) = unsafe { ADVANCED_PIC.as_mut() } {
        apic.add_ioapic(0xFEC00000);
    }

    Ok(())
}

/// Get advanced PIC instance
pub fn get_apic() -> Option<&'static mut AdvancedPic> {
    unsafe { ADVANCED_PIC.as_mut() }
}

/// Check if APIC is available
pub fn is_apic_available() -> bool {
    unsafe { LocalApic::new().is_some() }
}

/// Disable legacy PIC (8259)
pub fn disable_legacy_pic() {
    unsafe {
        // Mask all interrupts on both PICs
        Port::<u8>::new(0x21).write(0xFF); // Master PIC data
        Port::<u8>::new(0xA1).write(0xFF); // Slave PIC data

        // Send ICW4 to both PICs to set them to 8086 mode (but masked)
        Port::<u8>::new(0x20).write(0x11); // Master PIC command
        Port::<u8>::new(0xA0).write(0x11); // Slave PIC command
        Port::<u8>::new(0x21).write(0x20); // Master PIC data
        Port::<u8>::new(0xA1).write(0x28); // Slave PIC data
        Port::<u8>::new(0x21).write(0x04); // Master PIC data
        Port::<u8>::new(0xA1).write(0x02); // Slave PIC data
        Port::<u8>::new(0x21).write(0x01); // Master PIC data
        Port::<u8>::new(0xA1).write(0x01); // Slave PIC data

        // Mask all interrupts again
        Port::<u8>::new(0x21).write(0xFF);
        Port::<u8>::new(0xA1).write(0xFF);
    }
}