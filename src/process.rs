//! Userland process management and ELF loader
//!
//! Provides basic process creation, ELF binary loading, and execution.
//! Currently supports simple userland processes with identity-mapped memory.

use crate::syscall::Syscall;
use core::mem;

/// Process ID type
pub type Pid = u32;

/// Process state
#[derive(Debug, Clone, Copy)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Terminated,
}

/// Basic process structure
#[derive(Clone, Copy)]
pub struct Process {
    pub pid: Pid,
    pub state: ProcessState,
    pub entry_point: u64, // Virtual address of _start
    pub stack_top: u64,   // Top of user stack
    pub stack_bottom: u64, // Bottom of user stack
    pub memory_regions: [MemoryRegion; 16], // Fixed-size array for allocated memory regions
}

/// Memory region for a process
#[derive(Clone, Copy)]
pub struct MemoryRegion {
    pub start: u64,
    pub size: usize,
    pub permissions: MemoryPermissions,
}

/// Memory permissions
#[derive(Clone, Copy)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// ELF loader result
pub type ElfResult<T> = Result<T, ElfError>;

/// ELF loading errors
#[derive(Debug)]
pub enum ElfError {
    InvalidElfHeader,
    UnsupportedElfClass,
    UnsupportedElfType,
    NoEntryPoint,
    InvalidProgramHeader,
    MemoryAllocationFailed,
}

/// ELF header (simplified, 64-bit only)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ElfHeader {
    e_ident: [u8; 16],     // ELF identification
    e_type: u16,           // Object file type
    e_machine: u16,        // Machine type
    e_version: u32,        // Object file version
    e_entry: u64,          // Entry point address
    e_phoff: u64,          // Program header offset
    e_shoff: u64,          // Section header offset
    e_flags: u32,          // Processor-specific flags
    e_ehsize: u16,         // ELF header size
    e_phentsize: u16,      // Program header entry size
    e_phnum: u16,          // Number of program header entries
    e_shentsize: u16,      // Section header entry size
    e_shnum: u16,          // Number of section header entries
    e_shstrndx: u16,       // Section name string table index
}

/// Program header (simplified)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ProgramHeader {
    p_type: u32,    // Type of segment
    p_flags: u32,   // Segment attributes
    p_offset: u64,  // Offset in file
    p_vaddr: u64,   // Virtual address in memory
    p_paddr: u64,   // Physical address (ignored)
    p_filesz: u64,  // Size of segment in file
    p_memsz: u64,   // Size of segment in memory
    p_align: u64,   // Alignment
}

/// ELF constants
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ET_EXEC: u16 = 2;        // Executable file
const EM_X86_64: u16 = 62;     // x86-64 machine
const PT_LOAD: u32 = 1;        // Loadable segment
const PF_R: u32 = 0x4;         // Read permission
const PF_W: u32 = 0x2;         // Write permission
const PF_X: u32 = 0x1;         // Execute permission

/// Global process list
static mut PROCESSES: [Option<Process>; 16] = [const { None }; 16];
static mut NEXT_PID: Pid = 1;

/// Initialize process management
pub fn init() {
    unsafe {
        PROCESSES = [const { None }; 16];
        NEXT_PID = 1;
    }
}

/// Load a userland function as a process (simplified for testing)
pub fn load_userland_function(entry_point: u64) -> ElfResult<Pid> {
    // Allocate process
    let pid = unsafe {
        let pid = NEXT_PID;
        NEXT_PID += 1;
        pid
    };

    let mut process = Process {
        pid,
        state: ProcessState::Ready,
        entry_point,
        stack_top: 0,
        stack_bottom: 0,
        memory_regions: [MemoryRegion { start: 0, size: 0, permissions: MemoryPermissions { read: false, write: false, execute: false } }; 16],
    };

    // Allocate user stack (4KB for now)
    allocate_user_stack(&mut process)?;

    // Add to process list
    unsafe {
        let processes = &mut *core::ptr::addr_of_mut!(PROCESSES);
        for slot in processes.iter_mut() {
            if slot.is_none() {
                *slot = Some(process);
                // Add to scheduler
                if let Some(scheduler) = crate::scheduler::get_scheduler().lock().as_mut() {
                    scheduler.add_process(process, 0); // Default priority 0
                }
                return Ok(pid);
            }
        }
    }

    Err(ElfError::MemoryAllocationFailed) // No free slots
}

/// Load a program segment into memory
fn load_segment(process: &mut Process, ph: &ProgramHeader, binary: &[u8]) -> ElfResult<()> {
    let vaddr = ph.p_vaddr;
    let mem_size = ph.p_memsz as usize;
    let file_size = ph.p_filesz as usize;
    let offset = ph.p_offset as usize;

    // For now, assume identity mapping - allocate physical memory
    // In a real system, we'd allocate virtual memory and map it
    let phys_addr = allocate_memory(mem_size)? as u64;

    // Copy data from binary
    if file_size > 0 {
        if offset + file_size > binary.len() {
            return Err(ElfError::InvalidProgramHeader);
        }

        unsafe {
            core::ptr::copy_nonoverlapping(
                binary.as_ptr().add(offset),
                phys_addr as *mut u8,
                file_size,
            );
        }

        // Zero out the rest (BSS)
        if mem_size > file_size {
            unsafe {
                core::ptr::write_bytes(
                    (phys_addr as usize + file_size) as *mut u8,
                    0,
                    mem_size - file_size,
                );
            }
        }
    }

    // Record memory region (skip for now)
    // process.memory_regions[0] = MemoryRegion { ... }; // Would need to find free slot

    Ok(())
}

/// Allocate user stack for a process
fn allocate_user_stack(process: &mut Process) -> ElfResult<()> {
    const STACK_SIZE: usize = 4096;
    let stack_bottom = allocate_memory(STACK_SIZE)? as u64;
    let stack_top = stack_bottom + STACK_SIZE as u64;

    process.stack_bottom = stack_bottom;
    process.stack_top = stack_top;

    Ok(())
}

/// Simple memory allocation (placeholder - should use frame allocator)
/// For now, just bump allocate from a fixed region
fn allocate_memory(size: usize) -> ElfResult<usize> {
    // Placeholder: allocate from 1GB mark (avoiding kernel memory)
    // In real implementation, use proper frame allocator
    static mut NEXT_ALLOC: usize = 0x40000000; // 1GB

    unsafe {
        let addr = NEXT_ALLOC;
        NEXT_ALLOC += size;
        // Align to page boundary
        NEXT_ALLOC = (NEXT_ALLOC + 4095) & !4095;
        Ok(addr)
    }
}

/// Execute a process (switch to userland)
/// This is a simplified version - in reality, we'd set up proper context switching
pub fn execute_process(pid: Pid) -> ElfResult<()> {
    // Find the process
    let process = unsafe {
        let processes = &*core::ptr::addr_of!(PROCESSES);
        processes.iter().find(|p| p.as_ref().map(|pr| pr.pid) == Some(pid))
            .and_then(|p| p.as_ref())
            .ok_or(ElfError::InvalidProgramHeader)? // Wrong error, but close enough
    };

    // For now, just call the entry point directly
    // In a real system, we'd set up user registers and iretq
    let entry_fn: extern "C" fn() = unsafe { mem::transmute(process.entry_point) };
    entry_fn();

    Ok(())
}

/// Get current process (placeholder)
pub fn current_process() -> Option<&'static Process> {
    // For now, return the first process
    unsafe {
        let processes = &*core::ptr::addr_of!(PROCESSES);
        processes.iter().find(|p| p.is_some())?.as_ref()
    }
}

/// Syscall wrapper for write
pub fn syscall_write(buf: *const u8, count: usize) -> isize {
    unsafe {
        let result: u64;
        core::arch::asm!(
            "mov rax, {}",
            "mov rdi, 1",  // stdout
            "mov rsi, {}",
            "mov rdx, {}",
            "int 0x80",
            "mov {}, rax",
            in(reg) Syscall::Write as u64,
            in(reg) buf,
            in(reg) count,
            out(reg) result,
        );
        result as isize
    }
}