//! Intel 8254x (E1000) Ethernet Driver
//!
//! Provides basic Ethernet networking capabilities.
//! Foundation for implementing TCP/IP stack and network services.

use crate::pci::{PciDevice, class_codes, network_subclasses};
use core::ptr;

/// E1000 Register Offsets
const E1000_CTRL: usize = 0x0000;        // Device Control
const E1000_STATUS: usize = 0x0008;      // Device Status
const E1000_EECD: usize = 0x0010;        // EEPROM/Flash Control
const E1000_EERD: usize = 0x0014;        // EEPROM Read
const E1000_CTRL_EXT: usize = 0x0018;    // Extended Device Control
const E1000_MDIC: usize = 0x0020;        // MDI Control
const E1000_FCAL: usize = 0x0028;        // Flow Control Address Low
const E1000_FCAH: usize = 0x002C;        // Flow Control Address High
const E1000_FCT: usize = 0x0030;         // Flow Control Type
const E1000_VET: usize = 0x0038;         // VLAN Ether Type
const E1000_ICR: usize = 0x00C0;         // Interrupt Cause Read
const E1000_ICS: usize = 0x00C8;         // Interrupt Cause Set
const E1000_IMS: usize = 0x00D0;         // Interrupt Mask Set/Read
const E1000_IMC: usize = 0x00D8;         // Interrupt Mask Clear
const E1000_RCTL: usize = 0x0100;        // Receive Control
const E1000_FCTTV: usize = 0x0170;       // Flow Control Transmit Timer Value
const E1000_TXCW: usize = 0x0178;        // Transmit Configuration Word
const E1000_RXCW: usize = 0x0180;        // Receive Configuration Word
const E1000_TCTL: usize = 0x0400;        // Transmit Control
const E1000_TIPG: usize = 0x0410;        // Transmit IPG
const E1000_AIFS: usize = 0x0458;        // Adaptive IFS Throttle
const E1000_LEDCTL: usize = 0x0E00;      // LED Control
const E1000_PBA: usize = 0x1000;         // Packet Buffer Allocation
const E1000_RDBAL: usize = 0x2800;       // Receive Descriptor Base Address Low
const E1000_RDBAH: usize = 0x2804;       // Receive Descriptor Base Address High
const E1000_RDLEN: usize = 0x2808;       // Receive Descriptor Length
const E1000_RDH: usize = 0x2810;         // Receive Descriptor Head
const E1000_RDT: usize = 0x2818;         // Receive Descriptor Tail
const E1000_RDTR: usize = 0x2820;        // Receive Delay Timer
const E1000_RADV: usize = 0x282C;        // Receive Interrupt Absolute Delay Timer
const E1000_TDBAL: usize = 0x3800;       // Transmit Descriptor Base Address Low
const E1000_TDBAH: usize = 0x3804;       // Transmit Descriptor Base Address High
const E1000_TDLEN: usize = 0x3808;       // Transmit Descriptor Length
const E1000_TDH: usize = 0x3810;         // Transmit Descriptor Head
const E1000_TDT: usize = 0x3818;         // Transmit Descriptor Tail
const E1000_TIDV: usize = 0x3820;        // Transmit Interrupt Delay Value
const E1000_TXDCTL: usize = 0x3828;      // Transmit Descriptor Control
const E1000_TADV: usize = 0x382C;        // Transmit Absolute Interrupt Delay Value
const E1000_MTA: usize = 0x5200;         // Multicast Table Array
const E1000_RAL: usize = 0x5400;         // Receive Address Low
const E1000_RAH: usize = 0x5404;         // Receive Address High

/// Receive Descriptor
#[repr(C)]
struct RxDescriptor {
    buffer_addr: u64,    // Buffer address
    length: u16,         // Length
    checksum: u16,       // Checksum
    status: u8,          // Status
    errors: u8,          // Errors
    special: u16,        // Special
}

/// Transmit Descriptor
#[repr(C)]
struct TxDescriptor {
    buffer_addr: u64,    // Buffer address
    length: u16,         // Length
    cso: u8,             // Checksum offset
    cmd: u8,             // Command
    status: u8,          // Status
    css: u8,             // Checksum start
    special: u16,        // Special
}

/// Ethernet frame buffer
const FRAME_SIZE: usize = 1518;
const RX_RING_SIZE: usize = 32;
const TX_RING_SIZE: usize = 32;

/// E1000 Ethernet Controller
pub struct E1000Controller {
    base_addr: u64,
    mac_addr: [u8; 6],
    rx_ring: [RxDescriptor; RX_RING_SIZE],
    tx_ring: [TxDescriptor; TX_RING_SIZE],
    rx_buffers: [[u8; FRAME_SIZE]; RX_RING_SIZE],
    tx_buffers: [[u8; FRAME_SIZE]; TX_RING_SIZE],
    rx_cur: usize,
    tx_cur: usize,
}

impl E1000Controller {
    /// Create E1000 controller from PCI device
    pub fn new(pci_device: &PciDevice) -> Result<Self, &'static str> {
        if pci_device.class != class_codes::NETWORK || pci_device.subclass != network_subclasses::ETHERNET {
            return Err("Not an Ethernet controller");
        }

        let base_addr = pci_device.get_bar(0)
            .ok_or("No Ethernet BAR found")?.0;

        // Enable PCI device
        pci_device.enable_bus_mastering();
        pci_device.enable_memory_space();

        let mut controller = E1000Controller {
            base_addr,
            mac_addr: [0; 6],
            rx_ring: [RxDescriptor {
                buffer_addr: 0,
                length: 0,
                checksum: 0,
                status: 0,
                errors: 0,
                special: 0,
            }; RX_RING_SIZE],
            tx_ring: [TxDescriptor {
                buffer_addr: 0,
                length: 0,
                cso: 0,
                cmd: 0,
                status: 0,
                css: 0,
                special: 0,
            }; TX_RING_SIZE],
            rx_buffers: [[0; FRAME_SIZE]; RX_RING_SIZE],
            tx_buffers: [[0; FRAME_SIZE]; TX_RING_SIZE],
            rx_cur: 0,
            tx_cur: 0,
        };

        controller.initialize()?;
        Ok(controller)
    }

    /// Initialize the E1000 controller
    fn initialize(&mut self) -> Result<(), &'static str> {
        unsafe {
            // Reset the device
            self.write_reg(E1000_CTRL, self.read_reg(E1000_CTRL) | (1 << 26));
            // Wait for reset to complete
            while (self.read_reg(E1000_CTRL) & (1 << 26)) != 0 {}

            // Read MAC address from EEPROM
            self.read_mac_address()?;

            // Disable interrupts
            self.write_reg(E1000_IMC, 0xFFFFFFFF);

            // Setup receive ring
            self.setup_receive_ring()?;

            // Setup transmit ring
            self.setup_transmit_ring()?;

            // Configure receive control
            self.write_reg(E1000_RCTL, (1 << 1) | (1 << 3) | (1 << 4)); // EN | SBP | UPE

            // Configure transmit control
            self.write_reg(E1000_TCTL, (1 << 1) | (1 << 3)); // EN | PSP
            self.write_reg(E1000_TIPG, 0x0060200A); // IPGT=10, IPGR1=8, IPGR2=6

            // Set MAC address
            self.write_reg(E1000_RAL, (self.mac_addr[0] as u32) |
                          ((self.mac_addr[1] as u32) << 8) |
                          ((self.mac_addr[2] as u32) << 16) |
                          ((self.mac_addr[3] as u32) << 24));
            self.write_reg(E1000_RAH, (self.mac_addr[4] as u32) |
                          ((self.mac_addr[5] as u32) << 8) | (1 << 31)); // AV = 1

            // Enable interrupts
            self.write_reg(E1000_IMS, (1 << 7) | (1 << 6) | (1 << 4)); // LSC | RXSEQ | RXDMT0

            // Start device
            self.write_reg(E1000_CTRL, self.read_reg(E1000_CTRL) | (1 << 6)); // SLU
        }

        Ok(())
    }

    /// Read MAC address from EEPROM
    fn read_mac_address(&mut self) -> Result<(), &'static str> {
        // Try to read from EEPROM
        for i in 0..3 {
            let word = self.read_eeprom(i)?;
            self.mac_addr[i * 2] = (word & 0xFF) as u8;
            self.mac_addr[i * 2 + 1] = ((word >> 8) & 0xFF) as u8;
        }
        Ok(())
    }

    /// Read EEPROM word
    fn read_eeprom(&self, address: u32) -> Result<u16, &'static str> {
        unsafe {
            // Request EEPROM read
            self.write_reg(E1000_EERD, (address << 8) | 1);

            // Wait for completion
            let mut timeout = 10000;
            while timeout > 0 {
                let eerd = self.read_reg(E1000_EERD);
                if (eerd & (1 << 4)) != 0 {
                    return Ok((eerd >> 16) as u16);
                }
                timeout -= 1;
            }
        }
        Err("EEPROM read timeout")
    }

    /// Setup receive descriptor ring
    fn setup_receive_ring(&mut self) -> Result<(), &'static str> {
        // Initialize descriptors
        for i in 0..RX_RING_SIZE {
            self.rx_ring[i].buffer_addr = self.rx_buffers[i].as_ptr() as u64;
            self.rx_ring[i].status = 0;
        }

        unsafe {
            // Set ring base address
            self.write_reg(E1000_RDBAL, self.rx_ring.as_ptr() as u32);
            self.write_reg(E1000_RDBAH, (self.rx_ring.as_ptr() as u64 >> 32) as u32);

            // Set ring length
            self.write_reg(E1000_RDLEN, (RX_RING_SIZE as u32) * 16);

            // Set head and tail
            self.write_reg(E1000_RDH, 0);
            self.write_reg(E1000_RDT, (RX_RING_SIZE - 1) as u32);
        }

        Ok(())
    }

    /// Setup transmit descriptor ring
    fn setup_transmit_ring(&mut self) -> Result<(), &'static str> {
        // Initialize descriptors
        for i in 0..TX_RING_SIZE {
            self.tx_ring[i].buffer_addr = self.tx_buffers[i].as_ptr() as u64;
            self.tx_ring[i].status = 1; // Descriptor empty
            self.tx_ring[i].cmd = 0;
        }

        unsafe {
            // Set ring base address
            self.write_reg(E1000_TDBAL, self.tx_ring.as_ptr() as u32);
            self.write_reg(E1000_TDBAH, (self.tx_ring.as_ptr() as u64 >> 32) as u32);

            // Set ring length
            self.write_reg(E1000_TDLEN, (TX_RING_SIZE as u32) * 16);

            // Set head and tail
            self.write_reg(E1000_TDH, 0);
            self.write_reg(E1000_TDT, 0);
        }

        Ok(())
    }

    /// Read register
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { ptr::read_volatile((self.base_addr + offset as u64) as *const u32) }
    }

    /// Write register
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe { ptr::write_volatile((self.base_addr + offset as u64) as *mut u32, value) }
    }

    /// Send Ethernet frame
    pub fn send_frame(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > FRAME_SIZE {
            return Err("Frame too large");
        }

        let desc_idx = self.tx_cur;
        let buffer = &mut self.tx_buffers[desc_idx];

        // Copy data to buffer
        buffer[..data.len()].copy_from_slice(data);
        for i in data.len()..buffer.len() {
            buffer[i] = 0;
        }

        // Update descriptor
        self.tx_ring[desc_idx].length = data.len() as u16;
        self.tx_ring[desc_idx].cmd = (1 << 3) | (1 << 0); // IDE | EOP
        self.tx_ring[desc_idx].status = 0; // Clear status

        // Update tail pointer
        self.tx_cur = (self.tx_cur + 1) % TX_RING_SIZE;
        unsafe {
            self.write_reg(E1000_TDT, self.tx_cur as u32);
        }

        Ok(())
    }

    /// Receive Ethernet frame
    pub fn receive_frame(&mut self) -> Option<&[u8]> {
        let desc = &self.rx_ring[self.rx_cur];

        if (desc.status & (1 << 0)) != 0 { // DD bit set
            let length = desc.length as usize;
            let data = &self.rx_buffers[self.rx_cur][..length];

            // Clear status and update tail
            self.rx_ring[self.rx_cur].status = 0;
            self.rx_cur = (self.rx_cur + 1) % RX_RING_SIZE;

            unsafe {
                self.write_reg(E1000_RDT, ((self.rx_cur + RX_RING_SIZE - 1) % RX_RING_SIZE) as u32);
            }

            Some(data)
        } else {
            None
        }
    }

    /// Get MAC address
    pub fn mac_address(&self) -> &[u8; 6] {
        &self.mac_addr
    }

    /// Handle interrupt
    pub fn handle_interrupt(&mut self) {
        let icr = self.read_reg(E1000_ICR);

        if (icr & (1 << 6)) != 0 { // RXDMT0
            // Receive interrupt
            while let Some(frame) = self.receive_frame() {
                // Process received frame
                self.process_received_frame(frame);
            }
        }

        if (icr & (1 << 7)) != 0 { // LSC
            // Link status change
            crate::serial_write("Ethernet link status changed\n");
        }
    }

    /// Process received frame (placeholder)
    fn process_received_frame(&self, frame: &[u8]) {
        // TODO: Implement frame processing (ARP, IP, etc.)
        crate::serial_write(&format!("Received frame of {} bytes\n", frame.len()));
    }
}

/// Global E1000 controller instance
static mut E1000_CONTROLLER: Option<E1000Controller> = None;

/// Initialize E1000 Ethernet driver
pub fn init() {
    crate::serial_write("Initializing Ethernet driver...\n");
    // Find Ethernet controller via PCI
    if let Some(scanner) = crate::pci::get_scanner() {
        crate::serial_write("PCI scanner available for Ethernet init\n");
        for device in scanner.find_devices(class_codes::NETWORK, network_subclasses::ETHERNET) {
            crate::serial_write(&format!("Found Ethernet device: {:04x}:{:04x}\n", device.vendor_id, device.device_id));
            if let Ok(controller) = E1000Controller::new(device) {
                unsafe {
                    E1000_CONTROLLER = Some(controller);
                    crate::serial_write("E1000 Ethernet controller initialized\n");

                    if let Some(ctrl) = E1000_CONTROLLER.as_ref() {
                        let mac = ctrl.mac_address();
                        crate::serial_write(&format!("MAC Address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]));
                    }
                }
                break; // Use first Ethernet controller found
            } else {
                crate::serial_write("Failed to create E1000 controller\n");
            }
        }
    } else {
        crate::serial_write("No PCI scanner available for Ethernet init\n");
    }
    crate::serial_write("Ethernet driver initialization complete\n");
}

/// Get E1000 controller instance
pub fn get_controller() -> Option<&'static mut E1000Controller> {
    unsafe { E1000_CONTROLLER.as_mut() }
}

/// Test Ethernet functionality
pub fn test_ethernet() {
    if let Some(controller) = get_controller() {
        let mac = controller.mac_address();
        crate::serial_write(&format!("Ethernet MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]));
    } else {
        crate::serial_write("No Ethernet controller found\n");
    }
}