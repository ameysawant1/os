//! PS/2 Keyboard Driver
//!
//! Implements PS/2 keyboard input handling for legacy keyboard support.
//! Provides scancode reading and basic key event processing.

#![allow(dead_code)]

use x86_64::instructions::port::Port;

/// PS/2 Keyboard Scancode
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scancode(pub u8);

/// Key Event Type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    Pressed(Scancode),
    Released(Scancode),
}

/// PS/2 Keyboard Controller
pub struct Ps2Keyboard {
    data_port: u16,
    status_port: u16,
}

impl Ps2Keyboard {
    /// Create new PS/2 keyboard instance
    pub const fn new() -> Self {
        Ps2Keyboard {
            data_port: 0x60,   // PS/2 data port
            status_port: 0x64, // PS/2 status/command port
        }
    }

    /// Check if data is available to read
    pub fn data_available(&self) -> bool {
        let mut port = Port::<u8>::new(self.status_port);
        unsafe { (port.read() & 1) != 0 }
    }

    /// Check if keyboard buffer is full (can write command)
    pub fn can_write_command(&self) -> bool {
        let mut port = Port::<u8>::new(self.status_port);
        unsafe { (port.read() & 2) == 0 }
    }

    /// Read scancode from keyboard
    pub fn read_scancode(&self) -> Option<Scancode> {
        if self.data_available() {
            let mut port = Port::<u8>::new(self.data_port);
            Some(Scancode(unsafe { port.read() }))
        } else {
            None
        }
    }

    /// Wait for data with timeout
    fn wait_for_data(&self, timeout_ms: u32) -> bool {
        for _ in 0..timeout_ms {
            if self.data_available() {
                return true;
            }
            // Simple delay (very basic, not accurate timing)
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        false
    }

    /// Wait until we can write command with timeout
    fn wait_for_write(&self, timeout_ms: u32) -> bool {
        for _ in 0..timeout_ms {
            if self.can_write_command() {
                return true;
            }
            // Simple delay
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        false
    }

    /// Send command to keyboard
    pub fn send_command(&self, command: u8) -> Result<(), &'static str> {
        if !self.wait_for_write(100) {
            return Err("Timeout waiting to send command");
        }
        let mut port = Port::<u8>::new(self.status_port);
        unsafe { port.write(command) };
        Ok(())
    }

    /// Send data to keyboard
    pub fn send_data(&self, data: u8) -> Result<(), &'static str> {
        if !self.wait_for_write(100) {
            return Err("Timeout waiting to send data");
        }
        let mut port = Port::<u8>::new(self.data_port);
        unsafe { port.write(data) };
        Ok(())
    }

    /// Initialize PS/2 keyboard
    pub fn init(&mut self) -> Result<(), &'static str> {
        // Disable devices
        self.send_command(0xAD)?; // Disable keyboard
        self.send_command(0xA7)?; // Disable mouse

        // Flush output buffer
        while self.data_available() {
            let _ = self.read_scancode();
        }

        // Set controller configuration
        self.send_command(0x20)?; // Read controller configuration
        if !self.wait_for_data(100) {
            return Err("Timeout reading controller configuration");
        }
        let mut port = Port::<u8>::new(self.data_port);
        let config = unsafe { port.read() };
        self.send_command(0x60)?; // Write controller configuration
        self.send_data(config & 0xBC)?; // Disable interrupts and translation

        // Perform self-test
        self.send_command(0xAA)?;
        if !self.wait_for_data(100) {
            return Err("Timeout waiting for self-test result");
        }
        let mut port = Port::<u8>::new(self.data_port);
        let test_result = unsafe { port.read() };
        if test_result != 0x55 {
            return Err("PS/2 controller self-test failed");
        }

        // Enable keyboard
        self.send_command(0xAE)?;

        // Reset keyboard
        self.send_data(0xFF)?;
        if !self.wait_for_data(500) { // Longer timeout for keyboard reset
            return Err("Timeout waiting for keyboard reset response");
        }
        let mut port = Port::<u8>::new(self.data_port);
        let reset_result = unsafe { port.read() };
        if reset_result != 0xAA {
            return Err("Keyboard reset failed");
        }

        // Set keyboard to default state
        self.send_data(0xF0)?; // Set scancode set
        self.send_data(0x02)?; // Scancode set 2

        Ok(())
    }

    /// Read key event (pressed/released)
    pub fn read_key_event(&self) -> Option<KeyEvent> {
        if let Some(scancode) = self.read_scancode() {
            if scancode.0 & 0x80 != 0 {
                // Break code (key released)
                Some(KeyEvent::Released(Scancode(scancode.0 & 0x7F)))
            } else {
                // Make code (key pressed)
                Some(KeyEvent::Pressed(scancode))
            }
        } else {
            None
        }
    }
}

/// Global PS/2 keyboard instance
static mut PS2_KEYBOARD: Option<Ps2Keyboard> = None;

/// Initialize PS/2 keyboard driver
pub fn init() -> Result<(), &'static str> {
    let mut keyboard = Ps2Keyboard::new();
    keyboard.init()?;

    unsafe {
        PS2_KEYBOARD = Some(keyboard);
    }

    // TODO: Register interrupt handler for IRQ 1 (keyboard)
    // crate::interrupts::register_handler(0x21, keyboard_interrupt_handler);

    Ok(())
}

/// Get PS/2 keyboard instance
#[allow(static_mut_refs)]
pub fn get_keyboard() -> Option<&'static mut Ps2Keyboard> {
    // Access the static mutable Option and return a mutable reference if initialized
    unsafe { PS2_KEYBOARD.as_mut() }
}

/// Keyboard interrupt handler
fn keyboard_interrupt_handler() {
    if let Some(keyboard) = get_keyboard() {
        if let Some(event) = keyboard.read_key_event() {
            // Process key event
            match event {
                KeyEvent::Pressed(scancode) => {
                    // Handle key press
                    handle_key_press(scancode);
                }
                KeyEvent::Released(scancode) => {
                    // Handle key release
                    handle_key_release(scancode);
                }
            }
        }
    }

    // Send EOI (End of Interrupt)
    let mut port = Port::<u8>::new(0x20);
    unsafe { port.write(0x20) }; // Master PIC EOI
}

/// Handle key press event
fn handle_key_press(scancode: Scancode) {
    // Basic scancode to ASCII conversion (simplified)
    let ascii = match scancode.0 {
        0x1E => Some(b'a'),
        0x30 => Some(b'b'),
        0x2E => Some(b'c'),
        // Add more mappings as needed
        0x39 => Some(b' '), // Space
        0x1C => Some(b'\n'), // Enter
        _ => None,
    };

    if let Some(_char) = ascii {
        // For now, just acknowledge key press
        // You can extend this to buffer input or send to console
    }
}

/// Handle key release event
fn handle_key_release(_scancode: Scancode) {
    // Handle key release if needed
}

/// Test PS/2 keyboard functionality
pub fn test_ps2_keyboard() {
    if let Some(_keyboard) = get_keyboard() {
        // Test reading scancodes
        // For now, just check if initialized
    } else {
        // Not initialized
    }
}