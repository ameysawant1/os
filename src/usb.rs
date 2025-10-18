//! USB Host Controller Driver (UHCI/EHCI/XHCI)
//!
//! Foundation for USB keyboard, mouse, and storage support.

use crate::pci::{PciDevice, class_codes, serial_write};

/// USB Controller Types
#[derive(Debug, Clone, Copy)]
pub enum UsbControllerType {
    Uhci,
    Ehci,
    Xhci,
    Unknown,
}

/// USB Controller
pub struct UsbController {
    pub pci_device: PciDevice,
    pub controller_type: UsbControllerType,
    pub base_addr: u64,
}

impl UsbController {
    pub fn new(pci_device: &PciDevice) -> Option<Self> {
        let controller_type = match pci_device.prog_if {
            0x00 => UsbControllerType::Uhci, // UHCI
            0x20 => UsbControllerType::Ehci, // EHCI
            0x30 => UsbControllerType::Xhci, // XHCI
            _ => UsbControllerType::Unknown,
        };
        let base_addr = pci_device.get_bar(0)?.0;
        Some(UsbController {
            pci_device: *pci_device,
            controller_type,
            base_addr,
        })
    }

    pub fn initialize(&self) {
        serial_write(&format!("USB Controller {:?} at {:x}\n", self.controller_type, self.base_addr));
        // TODO: Implement controller-specific initialization
    }
}

/// Global USB controller list
static mut USB_CONTROLLERS: [Option<UsbController>; 8] = [None; 8];
static mut USB_CONTROLLER_COUNT: usize = 0;

/// Initialize USB drivers
pub fn init() {
    serial_write("Initializing USB drivers...\n");
    if let Some(scanner) = crate::pci::get_scanner() {
        for device in scanner.find_devices(class_codes::SERIAL_BUS, 0x03) { // 0x03 = USB subclass
            if let Some(controller) = UsbController::new(device) {
                unsafe {
                    if USB_CONTROLLER_COUNT < USB_CONTROLLERS.len() {
                        USB_CONTROLLERS[USB_CONTROLLER_COUNT] = Some(controller);
                        USB_CONTROLLER_COUNT += 1;
                        controller.initialize();
                    }
                }
            }
        }
    }
    serial_write("USB driver initialization complete\n");
}

/// Get USB controller list
pub fn get_controllers() -> &'static mut [Option<UsbController>; 8] {
    unsafe { &mut USB_CONTROLLERS }
}
