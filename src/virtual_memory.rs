//! Virtual Memory Management
//!
//! Implements proper paging using the x86_64 crate.
//! Provides memory protection, virtual address spaces, and enables advanced heap allocation.

use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PhysFrame, Size4KiB,
    PageTableFlags, Translate,
};
use x86_64::{PhysAddr, VirtAddr};

/// Virtual memory manager
pub struct VirtualMemoryManager {
    mapper: OffsetPageTable<'static>,
}

impl VirtualMemoryManager {
    /// Create a new virtual memory manager
    ///
    /// # Safety
    /// This function is unsafe because it assumes the physical memory offset is correct
    pub unsafe fn new(physical_memory_offset: VirtAddr) -> Self {
        unsafe {
            let level_4_table = active_level_4_table(physical_memory_offset);
            let mapper = OffsetPageTable::new(level_4_table, physical_memory_offset);

            VirtualMemoryManager {
                mapper,
            }
        }
    }

    /// Map a page to a frame with given flags
    pub fn map_page(
        &mut self,
        page: Page,
        frame: PhysFrame,
        flags: PageTableFlags,
    ) -> Result<(), &'static str> {
        // Create a temporary frame allocator wrapper
        struct TempFrameAllocator;
        unsafe impl FrameAllocator<Size4KiB> for TempFrameAllocator {
            fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
                crate::frame_allocator::allocate_frame()
                    .map(|f| PhysFrame::from_start_address(f.start).unwrap())
            }
        }

        let mut temp_allocator = TempFrameAllocator;
        unsafe {
            self.mapper.map_to(page, frame, flags, &mut temp_allocator)
                .map_err(|_| "Failed to map page")?
                .flush();
        }
        Ok(())
    }

    /// Unmap a page
    pub fn unmap_page(&mut self, page: Page) -> Result<(), &'static str> {
        self.mapper.unmap(page)
            .map_err(|_| "Failed to unmap page")?;
        Ok(())
    }

    /// Translate virtual address to physical address
    pub fn translate_addr(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.mapper.translate_addr(addr)
    }

    /// Allocate and map a page
    pub fn allocate_page(&mut self, flags: PageTableFlags) -> Result<Page, &'static str> {
        let frame = crate::frame_allocator::allocate_frame()
            .map(|f| PhysFrame::from_start_address(f.start).unwrap())
            .ok_or("No free frames available")?;
        let page = Page::containing_address(VirtAddr::new(frame.start_address().as_u64()));

        self.map_page(page, frame, flags)?;
        Ok(page)
    }

    /// Get a reference to the mapper
    pub fn mapper(&mut self) -> &mut OffsetPageTable<'static> {
        &mut self.mapper
    }
}

/// Get the active Level 4 page table
///
/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
/// It assumes that the CR3 register points to a valid Level 4 page table.
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

/// Initialize virtual memory management
/// This should be called after exiting boot services
pub fn init(physical_memory_offset: VirtAddr) -> VirtualMemoryManager {
    unsafe {
        VirtualMemoryManager::new(physical_memory_offset)
    }
}

/// Create identity mapping for the kernel
/// Maps the first 4GB of physical memory to virtual memory
pub fn create_identity_mapping(
    vmm: &mut VirtualMemoryManager,
    start_addr: PhysAddr,
    end_addr: PhysAddr,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let start_frame = PhysFrame::containing_address(start_addr);
    let end_frame = PhysFrame::containing_address(end_addr);

    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        let page = Page::containing_address(VirtAddr::new(frame.start_address().as_u64()));
        vmm.map_page(page, frame, flags)?;
    }

    Ok(())
}

/// Allocate kernel heap pages
/// This enables the advanced heap allocator
pub fn allocate_kernel_heap(
    vmm: &mut VirtualMemoryManager,
    heap_start: VirtAddr,
    heap_size: usize,
) -> Result<(), &'static str> {
    let num_pages = (heap_size + 4095) / 4096; // Round up to page boundary
    let start_page = Page::containing_address(heap_start);

    for i in 0..num_pages {
        let page = start_page + i as u64;
        let frame = vmm.frame_allocator.allocate_frame()
            .ok_or("No free frames for heap")?;

        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        vmm.map_page(page, frame, flags)?;
    }

    Ok(())
}

/// Test virtual memory functionality
#[cfg(test)]
pub fn test_virtual_memory() {
    // This would require a test environment with proper memory setup
    // For now, this is a placeholder
}