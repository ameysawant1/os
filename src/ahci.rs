//! AHCI (Advanced Host Controller Interface) SATA Driver
//!
//! Provides SATA disk access for storage operations.
//! Enables reading/writing to SATA drives for filesystem expansion.

use crate::pci::{PciDevice, class_codes, storage_subclasses};
use core::ptr;

/// AHCI Controller Registers
const AHCI_CAP: usize = 0x00;        // Host Capabilities
const AHCI_GHC: usize = 0x04;        // Global Host Control
const AHCI_IS: usize = 0x08;         // Interrupt Status
const AHCI_PI: usize = 0x0C;         // Ports Implemented
const AHCI_VS: usize = 0x10;         // Version
const AHCI_CCC_CTL: usize = 0x14;    // Command Completion Coalescing Control
const AHCI_CCC_PORTS: usize = 0x18;  // Command Completion Coalescing Ports
const AHCI_EM_LOC: usize = 0x1C;     // Enclosure Management Location
const AHCI_EM_CTL: usize = 0x20;     // Enclosure Management Control
const AHCI_CAP2: usize = 0x24;       // Host Capabilities Extended
const AHCI_BOHC: usize = 0x28;       // BIOS/OS Handoff Control and Status

/// Port Registers (relative to port base)
const PORT_CLB: usize = 0x00;        // Command List Base Address
const PORT_CLBU: usize = 0x04;       // Command List Base Address Upper 32-bits
const PORT_FB: usize = 0x08;         // FIS Base Address
const PORT_FBU: usize = 0x0C;        // FIS Base Address Upper 32-bits
const PORT_IS: usize = 0x10;         // Interrupt Status
const PORT_IE: usize = 0x14;         // Interrupt Enable
const PORT_CMD: usize = 0x18;        // Command and Status
const PORT_TFD: usize = 0x20;        // Task File Data
const PORT_SIG: usize = 0x24;        // Signature
const PORT_SSTS: usize = 0x28;       // Serial ATA Status
const PORT_SCTL: usize = 0x2C;       // Serial ATA Control
const PORT_SERR: usize = 0x30;       // Serial ATA Error
const PORT_SACT: usize = 0x34;       // Serial ATA Active
const PORT_CI: usize = 0x38;         // Command Issue
const PORT_SNTF: usize = 0x3C;       // Serial ATA Notification
const PORT_FBS: usize = 0x40;        // FIS-based Switching Control

/// AHCI Port States
#[derive(Debug, Clone, Copy)]
enum PortState {
    NoDevice,
    Present,
    Active,
}

/// AHCI Command Header
#[repr(C)]
struct CommandHeader {
    flags: u16,          // Command flags
    prdtl: u16,          // Physical Region Descriptor Table Length
    prdbc: u32,          // Physical Region Descriptor Byte Count
    ctba: u32,           // Command Table Base Address
    ctbau: u32,          // Command Table Base Address Upper 32-bits
    reserved: [u32; 4],  // Reserved
}

/// AHCI Command Table
#[repr(C)]
struct CommandTable {
    cfis: [u8; 64],      // Command FIS
    acmd: [u8; 16],      // ATAPI Command
    reserved: [u8; 48],  // Reserved
    prdt: [u8; 0],       // Physical Region Descriptor Table (variable size)
}

/// Physical Region Descriptor
#[repr(C)]
struct Prd {
    dba: u32,   // Data Base Address
    dbau: u32,  // Data Base Address Upper 32-bits
    reserved: u32,
    flags: u32, // Flags and byte count
}

/// AHCI Port
struct AhciPort {
    port_base: u64,
    state: PortState,
    command_list: Option<u64>,
    fis_base: Option<u64>,
}

impl AhciPort {
    fn new(port_base: u64) -> Self {
        AhciPort {
            port_base,
            state: PortState::NoDevice,
            command_list: None,
            fis_base: None,
        }
    }

    /// Check if port has a device
    fn probe(&mut self) {
        unsafe {
            let ssts = ptr::read_volatile((self.port_base + PORT_SSTS) as *const u32);
            let sig = ptr::read_volatile((self.port_base + PORT_SIG) as *const u32);

            // Check if device is present and detected
            let ipm = (ssts >> 8) & 0x0F;
            let det = ssts & 0x0F;

            if det == 0x03 && ipm == 0x01 {
                // Device present and active
                if sig == 0x00000101 {
                    self.state = PortState::Active; // SATA drive
                } else {
                    self.state = PortState::Present; // Other device
                }
            } else {
                self.state = PortState::NoDevice;
            }
        }
    }

    /// Start port
    fn start(&mut self) -> Result<(), &'static str> {
        unsafe {
            // Clear interrupts
            ptr::write_volatile((self.port_base + PORT_IS) as *mut u32, 0xFFFFFFFF);

            // Allocate command list and FIS receive area
            // TODO: Proper memory allocation
            self.command_list = Some(0x100000); // Placeholder address
            self.fis_base = Some(0x101000);     // Placeholder address

            if let (Some(cl), Some(fb)) = (self.command_list, self.fis_base) {
                // Set command list base
                ptr::write_volatile((self.port_base + PORT_CLB) as *mut u32, cl as u32);
                ptr::write_volatile((self.port_base + PORT_CLBU) as *mut u32, (cl >> 32) as u32);

                // Set FIS base
                ptr::write_volatile((self.port_base + PORT_FB) as *mut u32, fb as u32);
                ptr::write_volatile((self.port_base + PORT_FBU) as *mut u32, (fb >> 32) as u32);

                // Enable FIS receive
                let cmd = ptr::read_volatile((self.port_base + PORT_CMD) as *const u32);
                ptr::write_volatile((self.port_base + PORT_CMD) as *mut u32, cmd | (1 << 4));

                // Enable command engine
                let cmd = ptr::read_volatile((self.port_base + PORT_CMD) as *const u32);
                ptr::write_volatile((self.port_base + PORT_CMD) as *mut u32, cmd | (1 << 0));
            }
        }

        Ok(())
    }

    /// Stop port
    fn stop(&mut self) {
        unsafe {
            // Disable command engine
            let cmd = ptr::read_volatile((self.port_base + PORT_CMD) as *const u32);
            ptr::write_volatile((self.port_base + PORT_CMD) as *mut u32, cmd & !(1 << 0));

            // Disable FIS receive
            let cmd = ptr::read_volatile((self.port_base + PORT_CMD) as *const u32);
            ptr::write_volatile((self.port_base + PORT_CMD) as *mut u32, cmd & !(1 << 4));
        }
    }

    /// Read sectors from disk
    fn read_sectors(&self, start_sector: u64, sector_count: u8, buffer: &mut [u8]) -> Result<(), &'static str> {
        if !matches!(self.state, PortState::Active) {
            return Err("No active SATA device");
        }

        // TODO: Implement actual AHCI command construction and execution
        // This is a placeholder that would need:
        // 1. Build command FIS
        // 2. Set up PRDT
        // 3. Issue command
        // 4. Wait for completion

        Err("AHCI read not implemented")
    }

    /// Write sectors to disk
    fn write_sectors(&self, start_sector: u64, sector_count: u8, buffer: &[u8]) -> Result<(), &'static str> {
        if !matches!(self.state, PortState::Active) {
            return Err("No active SATA device");
        }

        // TODO: Implement actual AHCI command construction and execution
        Err("AHCI write not implemented")
    }
}

/// AHCI Controller
pub struct AhciController {
    base_addr: u64,
    ports: [Option<AhciPort>; 32],
    port_count: usize,
}

impl AhciController {
    /// Create AHCI controller from PCI device
    pub fn new(pci_device: &PciDevice) -> Result<Self, &'static str> {
        if pci_device.class != class_codes::STORAGE || pci_device.subclass != storage_subclasses::SATA {
            return Err("Not an AHCI SATA controller");
        }

        let base_addr = pci_device.get_bar(5)
            .ok_or("No AHCI BAR found")?.0;

        // Enable PCI device
        pci_device.enable_bus_mastering();
        pci_device.enable_memory_space();

        let mut controller = AhciController {
            base_addr,
            ports: [None; 32],
            port_count: 0,
        };

        controller.initialize()?;
        Ok(controller)
    }

    /// Initialize AHCI controller
    fn initialize(&mut self) -> Result<(), &'static str> {
        unsafe {
            // Reset controller
            let ghc = ptr::read_volatile((self.base_addr + AHCI_GHC) as *const u32);
            ptr::write_volatile((self.base_addr + AHCI_GHC) as *mut u32, ghc | (1 << 31)); // HBA reset

            // Wait for reset to complete
            loop {
                let ghc = ptr::read_volatile((self.base_addr + AHCI_GHC) as *const u32);
                if (ghc & (1 << 31)) == 0 {
                    break;
                }
            }

            // Enable AHCI
            ptr::write_volatile((self.base_addr + AHCI_GHC) as *mut u32, 1 << 31);

            // Get implemented ports
            let pi = ptr::read_volatile((self.base_addr + AHCI_PI) as *const u32);

            // Initialize ports
            for i in 0..32 {
                if (pi & (1 << i)) != 0 {
                    let port_base = self.base_addr + 0x100 + (i * 0x80) as u64;
                    let mut port = AhciPort::new(port_base);
                    port.probe();

                    if matches!(port.state, PortState::Active | PortState::Present) {
                        self.ports[self.port_count] = Some(port);
                        self.port_count += 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Get port by index
    pub fn get_port(&self, index: usize) -> Option<&AhciPort> {
        self.ports.get(index).and_then(|p| p.as_ref())
    }

    /// Get port count
    pub fn port_count(&self) -> usize {
        self.port_count
    }

    /// Start all ports
    pub fn start_ports(&mut self) -> Result<(), &'static str> {
        for port in self.ports.iter_mut().flatten() {
            port.start()?;
        }
        Ok(())
    }
}

/// Global AHCI controller instance
static mut AHCI_CONTROLLER: Option<AhciController> = None;

/// Initialize AHCI driver
pub fn init() {
    // Find AHCI controller via PCI
    if let Some(scanner) = crate::pci::get_scanner() {
        for device in scanner.find_devices(class_codes::STORAGE, storage_subclasses::SATA) {
            if let Ok(controller) = AhciController::new(device) {
                unsafe {
                    AHCI_CONTROLLER = Some(controller);
                    if let Some(ctrl) = AHCI_CONTROLLER.as_mut() {
                        if ctrl.start_ports().is_ok() {
                            crate::serial_write("AHCI controller initialized successfully\n");
                        }
                    }
                }
                break; // Use first AHCI controller found
            }
        }
    }
}

/// Get AHCI controller instance
pub fn get_controller() -> Option<&'static mut AhciController> {
    unsafe { AHCI_CONTROLLER.as_mut() }
}

/// Test AHCI functionality
pub fn test_ahci() {
    if let Some(controller) = get_controller() {
        crate::serial_write(&format!("AHCI controller has {} ports\n", controller.port_count()));

        for i in 0..controller.port_count() {
            if let Some(port) = controller.get_port(i) {
                crate::serial_write(&format!("Port {}: {:?}\n", i, port.state));
            }
        }
    } else {
        crate::serial_write("No AHCI controller found\n");
    }
}