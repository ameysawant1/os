//! PCI Device Driver Framework
//!
//! Provides PCI bus enumeration, device discovery, and driver management.
//! Foundation for storage, network, and other PCI device drivers.

use core::ptr;

/// PCI Configuration Space Registers
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// PCI Configuration Space Offsets
const PCI_VENDOR_ID: u8 = 0x00;
const PCI_DEVICE_ID: u8 = 0x02;
const PCI_COMMAND: u8 = 0x04;
const PCI_STATUS: u8 = 0x06;
const PCI_REVISION_ID: u8 = 0x08;
const PCI_PROG_IF: u8 = 0x09;
const PCI_SUBCLASS: u8 = 0x0A;
const PCI_CLASS: u8 = 0x0B;
const PCI_CACHE_LINE_SIZE: u8 = 0x0C;
const PCI_LATENCY_TIMER: u8 = 0x0D;
const PCI_HEADER_TYPE: u8 = 0x0E;
const PCI_BIST: u8 = 0x0F;
const PCI_BAR0: u8 = 0x10;
const PCI_BAR1: u8 = 0x14;
const PCI_BAR2: u8 = 0x18;
const PCI_BAR3: u8 = 0x1C;
const PCI_BAR4: u8 = 0x20;
const PCI_BAR5: u8 = 0x24;
const PCI_INTERRUPT_LINE: u8 = 0x3C;
const PCI_INTERRUPT_PIN: u8 = 0x3D;

/// PCI Device structure
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub header_type: u8,
    pub bars: [u32; 6],
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

impl PciDevice {
    /// Check if device is present
    pub fn is_present(&self) -> bool {
        self.vendor_id != 0xFFFF
    }

    /// Check if device is multifunction
    pub fn is_multifunction(&self) -> bool {
        (self.header_type & 0x80) != 0
    }

    /// Get BAR address and size
    pub fn get_bar(&self, index: usize) -> Option<(u64, usize)> {
        if index >= self.bars.len() {
            return None;
        }

        let bar = self.bars[index];
        if bar == 0 {
            return None;
        }

        // Check if BAR is memory-mapped or I/O port
        if (bar & 1) == 0 {
            // Memory BAR
            let addr = (bar & !0xF) as u64;
            // TODO: Calculate size by writing all 1s and reading back
            let size = 4096; // Placeholder - should calculate actual size
            Some((addr, size))
        } else {
            // I/O BAR
            let port = (bar & !0x3) as u16;
            let size = 256; // Placeholder
            Some((port as u64, size))
        }
    }

    /// Enable bus mastering
    pub fn enable_bus_mastering(&self) {
        let mut command = self.read_config_word(PCI_COMMAND);
        command |= 1 << 2; // Set bus master bit
        self.write_config_word(PCI_COMMAND, command);
    }

    /// Enable memory space
    pub fn enable_memory_space(&self) {
        let mut command = self.read_config_word(PCI_COMMAND);
        command |= 1 << 1; // Set memory space bit
        self.write_config_word(PCI_COMMAND, command);
    }

    /// Enable I/O space
    pub fn enable_io_space(&self) {
        let mut command = self.read_config_word(PCI_COMMAND);
        command |= 1 << 0; // Set I/O space bit
        self.write_config_word(PCI_COMMAND, command);
    }

    /// Read configuration byte
    pub fn read_config_byte(&self, offset: u8) -> u8 {
        pci_config_read_byte(self.bus, self.device, self.function, offset)
    }

    /// Read configuration word
    pub fn read_config_word(&self, offset: u8) -> u16 {
        pci_config_read_word(self.bus, self.device, self.function, offset)
    }

    /// Read configuration dword
    pub fn read_config_dword(&self, offset: u8) -> u32 {
        pci_config_read_dword(self.bus, self.device, self.function, offset)
    }

    /// Write configuration word
    pub fn write_config_word(&self, offset: u8, value: u16) {
        pci_config_write_word(self.bus, self.device, self.function, offset, value);
    }
}

/// PCI Bus Scanner
pub struct PciScanner {
    devices: [Option<PciDevice>; 256], // Support up to 256 devices
    device_count: usize,
}

impl PciScanner {
    pub fn new() -> Self {
        PciScanner {
            devices: [None; 256],
            device_count: 0,
        }
    }

    /// Scan all PCI buses and devices
    pub fn scan(&mut self) {
        for bus in 0..256u8 {
            for device in 0..32u8 {
                // Check function 0 first
                if let Some(pci_device) = self.probe_device(bus, device, 0) {
                    self.add_device(pci_device);

                    // If multifunction, check other functions
                    if pci_device.is_multifunction() {
                        for function in 1..8u8 {
                            if let Some(func_device) = self.probe_device(bus, device, function) {
                                self.add_device(func_device);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Probe a specific PCI device
    fn probe_device(&self, bus: u8, device: u8, function: u8) -> Option<PciDevice> {
        let vendor_id = pci_config_read_word(bus, device, function, PCI_VENDOR_ID);
        if vendor_id == 0xFFFF {
            return None; // Device not present
        }

        let device_id = pci_config_read_word(bus, device, function, PCI_DEVICE_ID);
        let class = pci_config_read_byte(bus, device, function, PCI_CLASS);
        let subclass = pci_config_read_byte(bus, device, function, PCI_SUBCLASS);
        let prog_if = pci_config_read_byte(bus, device, function, PCI_PROG_IF);
        let revision = pci_config_read_byte(bus, device, function, PCI_REVISION_ID);
        let header_type = pci_config_read_byte(bus, device, function, PCI_HEADER_TYPE);

        let mut bars = [0u32; 6];
        for i in 0..6 {
            bars[i] = pci_config_read_dword(bus, device, function, PCI_BAR0 + (i as u8 * 4));
        }

        let interrupt_line = pci_config_read_byte(bus, device, function, PCI_INTERRUPT_LINE);
        let interrupt_pin = pci_config_read_byte(bus, device, function, PCI_INTERRUPT_PIN);

        Some(PciDevice {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class,
            subclass,
            prog_if,
            revision,
            header_type,
            bars,
            interrupt_line,
            interrupt_pin,
        })
    }

    /// Add device to list
    fn add_device(&mut self, device: PciDevice) {
        if self.device_count < self.devices.len() {
            self.devices[self.device_count] = Some(device);
            self.device_count += 1;
        }
    }

    /// Get device by index
    pub fn get_device(&self, index: usize) -> Option<&PciDevice> {
        self.devices.get(index).and_then(|d| d.as_ref())
    }

    /// Get device count
    pub fn device_count(&self) -> usize {
        self.device_count
    }

    /// Find devices by class/subclass
    pub fn find_devices(&self, class: u8, subclass: u8) -> impl Iterator<Item = &PciDevice> {
        self.devices.iter().filter_map(|d| d.as_ref()).filter(move |d| d.class == class && d.subclass == subclass)
    }

    /// Find devices by vendor/device ID
    pub fn find_devices_by_id(&self, vendor_id: u16, device_id: u16) -> impl Iterator<Item = &PciDevice> {
        self.devices.iter().filter_map(|d| d.as_ref()).filter(move |d| d.vendor_id == vendor_id && d.device_id == device_id)
    }
}

/// PCI Configuration Space Access Functions
fn pci_config_read_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    pci_config_read_dword(bus, device, function, offset) as u8
}

fn pci_config_read_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    pci_config_read_dword(bus, device, function, offset) as u16
}

fn pci_config_read_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x80000000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & !3u32);

    unsafe {
        // Write address to CONFIG_ADDRESS
        ptr::write_volatile(PCI_CONFIG_ADDRESS as *mut u32, address);
        // Read data from CONFIG_DATA
        ptr::read_volatile(PCI_CONFIG_DATA as *const u32)
    }
}

fn pci_config_write_word(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let address = 0x80000000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & !3u32);

    let current = pci_config_read_dword(bus, device, function, offset & !3u8);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_value = (current & mask) | ((value as u32) << shift);

    unsafe {
        // Write address to CONFIG_ADDRESS
        ptr::write_volatile(PCI_CONFIG_ADDRESS as *mut u32, address);
        // Write data to CONFIG_DATA
        ptr::write_volatile(PCI_CONFIG_DATA as *mut u32, new_value);
    }
}

/// PCI Class Codes
pub mod class_codes {
    pub const STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const MULTIMEDIA: u8 = 0x04;
    pub const MEMORY: u8 = 0x05;
    pub const BRIDGE: u8 = 0x06;
    pub const COMMUNICATION: u8 = 0x07;
    pub const SYSTEM: u8 = 0x08;
    pub const INPUT: u8 = 0x09;
    pub const DOCKING: u8 = 0x0A;
    pub const PROCESSOR: u8 = 0x0B;
    pub const SERIAL: u8 = 0x0C;
    pub const WIRELESS: u8 = 0x0D;
    pub const INTELLIGENT_IO: u8 = 0x0E;
    pub const SATELLITE: u8 = 0x0F;
    pub const ENCRYPTION: u8 = 0x10;
    pub const SIGNAL_PROCESSING: u8 = 0x11;
    pub const PROCESSING_ACCELERATOR: u8 = 0x12;
    pub const NON_ESSENTIAL: u8 = 0x13;
}

/// PCI Subclass Codes for Storage
pub mod storage_subclasses {
    pub const SCSI: u8 = 0x00;
    pub const IDE: u8 = 0x01;
    pub const FLOPPY: u8 = 0x02;
    pub const IPI: u8 = 0x03;
    pub const RAID: u8 = 0x04;
    pub const ATA: u8 = 0x05;
    pub const SATA: u8 = 0x06;
    pub const SAS: u8 = 0x07;
    pub const NVME: u8 = 0x08;
    pub const UFS: u8 = 0x09;
}

/// PCI Subclass Codes for Network
pub mod network_subclasses {
    pub const ETHERNET: u8 = 0x00;
    pub const TOKEN_RING: u8 = 0x01;
    pub const FDDI: u8 = 0x02;
    pub const ATM: u8 = 0x03;
    pub const ISDN: u8 = 0x04;
    pub const WORLD_FLIP: u8 = 0x05;
    pub const PICMG_2_14: u8 = 0x06;
    pub const INFINIBAND: u8 = 0x07;
    pub const FABRIC: u8 = 0x08;
}

/// Global PCI scanner instance
static mut PCI_SCANNER: Option<PciScanner> = None;

/// Initialize PCI subsystem
pub fn init() {
    crate::serial_write("Initializing PCI bus enumeration...\n");
    unsafe {
        PCI_SCANNER = Some(PciScanner::new());
        crate::serial_write("PCI scanner created\n");
        if let Some(scanner) = PCI_SCANNER.as_mut() {
            scanner.scan();
            crate::serial_write("PCI scan completed\n");
        }
    }
    crate::serial_write("PCI initialization complete\n");
}

/// Get PCI scanner instance
pub fn get_scanner() -> Option<&'static mut PciScanner> {
    unsafe { PCI_SCANNER.as_mut() }
}

/// Print PCI device information
pub fn print_devices() {
    if let Some(scanner) = get_scanner() {
        crate::serial_write("PCI Devices Found:\n");
        for i in 0..scanner.device_count() {
            if let Some(device) = scanner.get_device(i) {
                crate::serial_write(&format!("  {:02x}:{:02x}.{:01x} {:04x}:{:04x} Class {:02x}:{:02x}:{:02x}\n",
                    device.bus, device.device, device.function,
                    device.vendor_id, device.device_id,
                    device.class, device.subclass, device.prog_if));
            }
        }
    }
}