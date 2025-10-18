//! Advanced Memory Allocator
//!
//! Uses linked_list_allocator for proper heap management with allocation/deallocation.
//! Replaces the simple bump allocator with a more sophisticated memory manager.

use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

/// Global heap allocator instance
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Heap start and size configuration
/// TODO: Make this configurable or auto-detected from UEFI memory map
const HEAP_START: usize = 0x_4444_4444_0000;
const HEAP_SIZE: usize = 100 * 1024; // 100 KiB

/// Initialize the heap allocator with allocated pages
pub fn init_heap_with_pages(heap_start: usize, heap_size: usize) -> Result<(), &'static str> {
    unsafe {
        ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
    }
    Ok(())
}

/// Allocate heap pages for the allocator itself
/// This needs to be called before init_heap() and requires paging to be set up
pub fn allocate_heap_pages<FA, M>(
    frame_allocator: &mut FA,
    mapper: &mut M,
) -> Result<(), &'static str>
where
    FA: FrameAllocator<Size4KiB>,
    M: Mapper<Size4KiB>,
{
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("Failed to allocate frame for heap")?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)
        }.map_err(|_| "Failed to map heap page")?
        .flush();
    }

    Ok(())
}

/// Get heap usage statistics
pub fn heap_usage() -> (usize, usize) {
    ALLOCATOR.lock().used() // Returns (used, total)
}

/// Test the allocator with some allocations
#[cfg(test)]
pub fn test_allocator() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    let heap_value = Box::new(41);
    assert_eq!(*heap_value, 41);

    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    assert_eq!(vec.len(), 500);
    assert_eq!(vec[0], 0);
    assert_eq!(vec[499], 499);
}