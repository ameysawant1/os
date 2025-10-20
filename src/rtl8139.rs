//! RTL8139 Ethernet Controller Driver
//!
//! Implements support for Realtek RTL8139 Ethernet controllers.
//! Provides basic network interface functionality for additional NIC support.

#![allow(dead_code)]

use x86_64::instructions::port::Port;

/// RTL8139 register offsets
const RTL8139_MAC: usize = 0x00;        // MAC address (6 bytes)
const RTL8139_MAR0: usize = 0x08;       // Multicast register 0
const RTL8139_RBSTART: usize = 0x30;    // Receive buffer start address
const RTL8139_CR: usize = 0x37;         // Command register
const RTL8139_IMR: usize = 0x3C;        // Interrupt mask register
const RTL8139_ISR: usize = 0x3E;        // Interrupt status register
const RTL8139_TCR: usize = 0x40;        // Transmit configuration register
const RTL8139_RCR: usize = 0x44;        // Receive configuration register
const RTL8139_TSAD0: usize = 0x20;      // Transmit start address descriptor 0
const RTL8139_TSD0: usize = 0x10;       // Transmit status descriptor 0

/// Command register bits
const CR_BUFE: u8 = 1 << 0;     // Buffer empty
const CR_TE: u8 = 1 << 2;       // Transmitter enable
const CR_RE: u8 = 1 << 3;       // Receiver enable
const CR_RST: u8 = 1 << 4;      // Reset

/// RTL8139 Ethernet Controller
pub struct Rtl8139Controller {
    base_addr: u64,
    mac_addr: [u8; 6],
    rx_buffer: &'static mut [u8; 8192 + 16], // 8KB + 16 bytes for alignment
    tx_buffer: &'static mut [u8; 1536],      // 1.5KB for transmit
}

impl Rtl8139Controller {
    /// Create RTL8139 controller from PCI device
    #[allow(static_mut_refs)]
    pub fn new(pci_device: &crate::pci::PciDevice) -> Result<Self, &'static str> {
        if pci_device.vendor_id != 0x10EC || pci_device.device_id != 0x8139 {
            return Err("Not an RTL8139 device");
        }

        let base_addr = (pci_device.bars[0] & 0xFFFFFFF0) as u64; // Get I/O base address

        // Allocate receive buffer (8KB aligned)
        let rx_buffer = unsafe {
            static mut RX_BUFFER: [u8; 8192 + 16] = [0; 8192 + 16];
            &mut RX_BUFFER
        };

        // Allocate transmit buffer
        let tx_buffer = unsafe {
            static mut TX_BUFFER: [u8; 1536] = [0; 1536];
            &mut TX_BUFFER
        };

        let mut controller = Rtl8139Controller {
            base_addr,
            mac_addr: [0; 6],
            rx_buffer,
            tx_buffer,
        };

        controller.init()?;
        Ok(controller)
    }

    /// Initialize the RTL8139 controller
    fn init(&mut self) -> Result<(), &'static str> {
        // Power on the device
        self.write_reg8(0x52, 0x00); // Power on

        // Software reset
        self.write_reg8(RTL8139_CR, CR_RST);
        while (self.read_reg8(RTL8139_CR) & CR_RST) != 0 {}

        // Read MAC address
        for i in 0..6 {
            self.mac_addr[i] = self.read_reg8(RTL8139_MAC + i);
        }

        // Set receive buffer
        let rx_buffer_addr = self.rx_buffer.as_ptr() as u32;
        self.write_reg32(RTL8139_RBSTART, rx_buffer_addr);

        // Set transmit configuration
        self.write_reg32(RTL8139_TCR, 0x03000600); // DMA burst size, etc.

        // Set receive configuration
        self.write_reg32(RTL8139_RCR, 0x0000070A); // Accept broadcast, multicast, physical match

        // Enable receiver and transmitter
        self.write_reg8(RTL8139_CR, CR_TE | CR_RE);

        // Clear interrupt status
        self.write_reg16(RTL8139_ISR, 0xFFFF);

        // Enable interrupts (optional)
        // self.write_reg16(RTL8139_IMR, 0x0005); // Transmit OK, Receive OK

        Ok(())
    }

    /// Read 8-bit register
    fn read_reg8(&self, offset: usize) -> u8 {
        let mut port = Port::<u8>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.read() }
    }

    /// Read 16-bit register
    fn read_reg16(&self, offset: usize) -> u16 {
        let mut port = Port::<u16>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.read() }
    }

    /// Read 32-bit register
    fn read_reg32(&self, offset: usize) -> u32 {
        let mut port = Port::<u32>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.read() }
    }

    /// Write 8-bit register
    fn write_reg8(&self, offset: usize, value: u8) {
        let mut port = Port::<u8>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.write(value) };
    }

    /// Write 16-bit register
    fn write_reg16(&self, offset: usize, value: u16) {
        let mut port = Port::<u16>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.write(value) };
    }

    /// Write 32-bit register
    fn write_reg32(&self, offset: usize, value: u32) {
        let mut port = Port::<u32>::new((self.base_addr + offset as u64) as u16);
        unsafe { port.write(value) };
    }

    /// Get MAC address
    pub fn mac_address(&self) -> &[u8; 6] {
        &self.mac_addr
    }

    /// Send packet
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > 1536 {
            return Err("Packet too large");
        }

        // Copy data to transmit buffer
        self.tx_buffer[..data.len()].copy_from_slice(data);

        // Set transmit start address
        let tx_addr = self.tx_buffer.as_ptr() as u32;
        self.write_reg32(RTL8139_TSAD0, tx_addr);

        // Set transmit status (packet length)
        self.write_reg32(RTL8139_TSD0, data.len() as u32);

        // Wait for transmission to complete (simple polling)
        while (self.read_reg32(RTL8139_TSD0) & 0x8000) == 0 {}

        Ok(())
    }

    /// Receive packet
    pub fn receive_packet(&mut self) -> Option<&[u8]> {
        // Check if packet is available
        let status = self.read_reg16(0); // Read from receive buffer
        if status == 0 {
            return None;
        }

        // Extract packet length
        let length = (status & 0x3FFF) as usize;

        if length > 0 && length <= 1536 {
            Some(&self.rx_buffer[4..4 + length]) // Skip header
        } else {
            None
        }
    }

    /// Handle interrupt
    pub fn handle_interrupt(&mut self) {
        let isr = self.read_reg16(RTL8139_ISR);
        if isr != 0 {
            // Clear interrupts
            self.write_reg16(RTL8139_ISR, isr);
        }
    }
}

/// Global RTL8139 controller instance
static mut RTL8139_CONTROLLER: Option<Rtl8139Controller> = None;

/// Initialize RTL8139 driver
pub fn init() -> Result<(), &'static str> {
    // Find RTL8139 device
    if let Some(scanner) = crate::pci::get_scanner() {
        for device in scanner.find_devices(crate::pci::class_codes::NETWORK, crate::pci::network_subclasses::ETHERNET) {
            if device.vendor_id == 0x10EC && device.device_id == 0x8139 {
                let controller = Rtl8139Controller::new(device)?;
                unsafe {
                    RTL8139_CONTROLLER = Some(controller);
                }
                return Ok(());
            }
        }
    }
    Err("RTL8139 device not found")
}

/// Get RTL8139 controller instance
#[allow(static_mut_refs)]
pub fn get_controller() -> Option<&'static mut Rtl8139Controller> {
    unsafe { RTL8139_CONTROLLER.as_mut() }
}

/// Test RTL8139 functionality
pub fn test_rtl8139() {
    if let Some(controller) = get_controller() {
        let _mac = controller.mac_address();
        // Print MAC address or test functionality
    } else {
        // Not initialized
    }
}