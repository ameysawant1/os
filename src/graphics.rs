/// Simple window primitive
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub color: u32,
}

/// Draw a window (rectangle) on the framebuffer
pub fn draw_window(win: &Window) {
    unsafe {
        if let Some(fb) = FRAMEBUFFER {
            for y in win.y..(win.y + win.height).min(fb.height) {
                for x in win.x..(win.x + win.width).min(fb.width) {
                    *fb.buffer.add(y * fb.stride + x) = win.color;
                }
            }
        }
    }
}
//! Graphics Driver (VGA/GOP/Framebuffer)
//!
//! Provides basic framebuffer and graphics primitives for GUI support.

use crate::serial_write;

/// Graphics mode
#[derive(Debug, Clone, Copy)]
pub enum GraphicsMode {
    Text,
    Framebuffer,
    GOP,
}

/// Framebuffer info
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub buffer: *mut u32,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

/// Global framebuffer info
static mut FRAMEBUFFER: Option<FramebufferInfo> = None;

/// Initialize graphics driver
pub fn init() {
    serial_write("Initializing graphics driver...\n");
    // TODO: Detect and initialize graphics mode (VGA, GOP, framebuffer)
    // For now, simulate framebuffer
    unsafe {
        FRAMEBUFFER = Some(FramebufferInfo {
            buffer: 0xB8000 as *mut u32, // Simulated address
            width: 640,
            height: 480,
            stride: 640,
        });
    }
    serial_write("Graphics driver initialized (simulated framebuffer)\n");
}

/// Draw a pixel
pub fn draw_pixel(x: usize, y: usize, color: u32) {
    unsafe {
        if let Some(fb) = FRAMEBUFFER {
            if x < fb.width && y < fb.height {
                *fb.buffer.add(y * fb.stride + x) = color;
            }
        }
    }
}

/// Clear screen
pub fn clear_screen(color: u32) {
    unsafe {
        if let Some(fb) = FRAMEBUFFER {
            for y in 0..fb.height {
                for x in 0..fb.width {
                    *fb.buffer.add(y * fb.stride + x) = color;
                }
            }
        }
    }
}
