//! Frame allocator for physical memory management
//!
//! Provides allocation and deallocation of physical memory frames using the UEFI memory map.
//! Includes PhysAddr and VirtAddr types for type safety.

use core::ops::{Add, Sub};
use uefi::mem::memory_map::MemoryDescriptor;

/// Physical address type for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        PhysAddr(addr)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl Add<u64> for PhysAddr {
    type Output = PhysAddr;

    fn add(self, rhs: u64) -> PhysAddr {
        PhysAddr(self.0 + rhs)
    }
}

impl Sub<u64> for PhysAddr {
    type Output = PhysAddr;

    fn sub(self, rhs: u64) -> PhysAddr {
        PhysAddr(self.0 - rhs)
    }
}

impl Sub<PhysAddr> for PhysAddr {
    type Output = u64;

    fn sub(self, rhs: PhysAddr) -> u64 {
        self.0 - rhs.0
    }
}

/// Virtual address type for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(u64);

impl VirtAddr {
    pub const fn new(addr: u64) -> Self {
        VirtAddr(addr)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl Add<u64> for VirtAddr {
    type Output = VirtAddr;

    fn add(self, rhs: u64) -> VirtAddr {
        VirtAddr(self.0 + rhs)
    }
}

impl Sub<u64> for VirtAddr {
    type Output = VirtAddr;

    fn sub(self, rhs: u64) -> VirtAddr {
        VirtAddr(self.0 - rhs)
    }
}

impl Sub<VirtAddr> for VirtAddr {
    type Output = u64;

    fn sub(self, rhs: VirtAddr) -> u64 {
        self.0 - rhs.0
    }
}

/// Frame size (4KB)
pub const FRAME_SIZE: u64 = 4096;

/// Frame represents a physical memory frame
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub start: PhysAddr,
}

impl Frame {
    pub fn containing_address(addr: PhysAddr) -> Frame {
        Frame {
            start: PhysAddr(addr.as_u64() & !(FRAME_SIZE - 1)),
        }
    }

    pub fn start_address(&self) -> PhysAddr {
        self.start
    }

    pub fn end_address(&self) -> PhysAddr {
        self.start + FRAME_SIZE
    }

    pub fn range_inclusive(start: Frame, end: Frame) -> FrameIter {
        FrameIter {
            start: start.start,
            end: end.start,
        }
    }
}

/// Iterator over frames
pub struct FrameIter {
    start: PhysAddr,
    end: PhysAddr,
}

impl Iterator for FrameIter {
    type Item = Frame;

    fn next(&mut self) -> Option<Frame> {
        if self.start >= self.end {
            None
        } else {
            let frame = Frame { start: self.start };
            self.start = self.start + FRAME_SIZE;
            Some(frame)
        }
    }
}

/// Frame allocator using bitmap
pub struct FrameAllocator {
    bitmap: &'static mut [u64], // Bitmap of allocated frames
    bitmap_start_frame: Frame,  // Frame where bitmap starts
    total_frames: usize,
    used_frames: usize,
}

impl FrameAllocator {
    /// Initialize frame allocator from UEFI memory map
    pub fn new(memory_map: &dyn uefi::mem::memory_map::MemoryMap) -> Option<Self> {
        // Find the largest conventional memory region for the bitmap
        let mut largest_region = None;
        let mut largest_size = 0;

        for descriptor in memory_map.entries() {
            if descriptor.ty == uefi::mem::memory_map::MemoryType::CONVENTIONAL {
                let size = descriptor.page_count as usize * 4096;
                if size > largest_size {
                    largest_size = size;
                    largest_region = Some(descriptor);
                }
            }
        }

        let region = largest_region?;
        let region_start = PhysAddr::new(region.phys_start);
        let region_size = region.page_count as u64 * 4096;

        // Reserve some frames for the bitmap itself
        // Calculate bitmap size: 1 bit per frame
        let total_frames_in_region = region_size / FRAME_SIZE;
        let bitmap_frames = (total_frames_in_region + 4095) / 4096; // Round up
        let bitmap_size = bitmap_frames * FRAME_SIZE;

        // Bitmap starts after reserved frames
        let bitmap_start = region_start + bitmap_size as u64;
        let bitmap_frame = Frame::containing_address(bitmap_start);

        // Initialize bitmap to all free (0)
        let bitmap_ptr = bitmap_start.as_mut_ptr::<u64>();
        let bitmap_len = (bitmap_size / 8) as usize; // 8 bits per u64
        unsafe {
            core::ptr::write_bytes(bitmap_ptr, 0, bitmap_len);
        }

        // Mark bitmap frames as allocated
        let bitmap_slice = unsafe {
            core::slice::from_raw_parts_mut(bitmap_ptr, bitmap_len)
        };

        for i in 0..bitmap_frames {
            let frame_index = i as usize;
            let byte_index = frame_index / 64;
            let bit_index = frame_index % 64;
            if byte_index < bitmap_slice.len() {
                bitmap_slice[byte_index] |= 1 << bit_index;
            }
        }

        Some(FrameAllocator {
            bitmap: bitmap_slice,
            bitmap_start_frame: bitmap_frame,
            total_frames: total_frames_in_region as usize,
            used_frames: bitmap_frames as usize,
        })
    }

    /// Allocate a frame
    pub fn allocate_frame(&mut self) -> Option<Frame> {
        for i in 0..self.total_frames {
            let byte_index = i / 64;
            let bit_index = i % 64;

            if byte_index >= self.bitmap.len() {
                continue;
            }

            if (self.bitmap[byte_index] & (1 << bit_index)) == 0 {
                // Frame is free
                self.bitmap[byte_index] |= 1 << bit_index;
                self.used_frames += 1;

                let frame_addr = self.bitmap_start_frame.start_address() + (i as u64 * FRAME_SIZE);
                return Some(Frame { start: frame_addr });
            }
        }
        None
    }

    /// Deallocate a frame
    pub fn deallocate_frame(&mut self, frame: Frame) {
        let frame_index = ((frame.start - self.bitmap_start_frame.start) / FRAME_SIZE) as usize;

        if frame_index >= self.total_frames {
            return; // Invalid frame
        }

        let byte_index = frame_index / 64;
        let bit_index = frame_index % 64;

        if byte_index < self.bitmap.len() {
            self.bitmap[byte_index] &= !(1 << bit_index);
            self.used_frames -= 1;
        }
    }

    /// Get statistics
    pub fn stats(&self) -> (usize, usize) {
        (self.used_frames, self.total_frames)
    }
}

/// Type alias for UEFI-compatible frame allocator
pub type UEFIFrameAllocator = FrameAllocator;

/// Initialize the global frame allocator
pub fn init(memory_map: &dyn uefi::mem::memory_map::MemoryMap) {
    unsafe {
        FRAME_ALLOCATOR = FrameAllocator::new(memory_map);
    }
}

/// Allocate a frame
pub fn allocate_frame() -> Option<Frame> {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(FRAME_ALLOCATOR);
        allocator.as_mut()?.allocate_frame()
    }
}

/// Deallocate a frame
pub fn deallocate_frame(frame: Frame) {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(FRAME_ALLOCATOR);
        if let Some(alloc) = allocator {
            alloc.deallocate_frame(frame);
        }
    }
}

/// Get allocator statistics
pub fn stats() -> Option<(usize, usize)> {
    unsafe {
        let allocator = &*core::ptr::addr_of!(FRAME_ALLOCATOR);
        allocator.as_ref().map(|a| a.stats())
    }
}