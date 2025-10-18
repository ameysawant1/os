//! Syscall ABI implementation for the UEFI OS kernel
//!
//! Provides the system call interface between userland processes and the kernel.
//! Currently supports basic syscalls like write() for serial output.

use crate::serial_write;
use x86_64::structures::idt::InterruptStackFrame;
use crate::filesystem::{Filesystem, FileDescriptor, OpenFlags, InodeNum};

/// Global filesystem instance
/// This will be initialized by the kernel during boot
pub static mut FILESYSTEM: *mut Filesystem = core::ptr::null_mut();

/// Syscall numbers
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum Syscall {
    Write = 0, // write(fd, buf, count) -> ssize_t
    Open = 1,  // open(path, flags, mode) -> fd
    Close = 2, // close(fd) -> int
    Read = 3,  // read(fd, buf, count) -> ssize_t
    CreateSnapshot = 4, // create_snapshot() -> int
    // Security syscalls
    SecurityCheck = 5,     // security_check(operation, level) -> bool
    AuditLog = 6,          // audit_log(operation, success, details) -> int
    RedactPII = 7,         // redact_pii(buf, len) -> int
    SetAutonomy = 8,       // set_autonomy(enabled) -> int
    KillSwitch = 9,        // kill_switch() -> int
    // Scheduler syscalls
    Yield = 10,            // yield() -> void
    Sleep = 11,            // sleep(ticks) -> int
    GetPid = 12,           // getpid() -> pid_t
    // Future syscalls can be added here
}

/// Syscall result type
pub type SyscallResult = Result<u64, SyscallError>;

/// Syscall error codes
#[repr(i64)]
#[derive(Debug, Clone, Copy)]
pub enum SyscallError {
    InvalidSyscall = -1,
    InvalidArgument = -2,
    PermissionDenied = -3,
    // Add more as needed
}

/// Convert filesystem error to syscall error
fn fs_error_to_syscall_error(err: crate::filesystem::FsError) -> SyscallError {
    match err {
        crate::filesystem::FsError::NoFreeInodes => SyscallError::InvalidArgument,
        crate::filesystem::FsError::NoFreeBlocks => SyscallError::InvalidArgument,
        crate::filesystem::FsError::NotRegularFile => SyscallError::InvalidArgument,
        crate::filesystem::FsError::FileTooLarge => SyscallError::InvalidArgument,
        crate::filesystem::FsError::DirectoryFull => SyscallError::InvalidArgument,
        crate::filesystem::FsError::FileNotFound => SyscallError::InvalidArgument,
        crate::filesystem::FsError::PermissionDenied => SyscallError::PermissionDenied,
    }
}

/// Syscall handler function
/// Called from the interrupt handler with syscall number and arguments
pub unsafe fn handle_syscall(
    syscall_num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _arg6: u64,
) -> SyscallResult {
    match syscall_num {
        x if x == Syscall::Write as u64 => {
            // write(fd, buf, count)
            let fd = arg1 as i32;
            let buf_ptr = arg2 as *const u8;
            let count = arg3 as usize;

            // Security check for data export
            if fd != 1 { // Not stdout
                if let Some(sm) = crate::security::get_security_manager() {
                    if let Ok(false) = sm.check_operation(crate::security::OperationType::DataExport, crate::security::SecurityLevel::Low) {
                        return Err(SyscallError::PermissionDenied);
                    }
                }
            }

            // Safety: We trust the userland pointer for now
            // In a real system, we'd validate the pointer range
            let buf_slice = unsafe { core::slice::from_raw_parts(buf_ptr, count) };

            if fd == 1 { // stdout (serial)
                // Convert to string (assuming UTF-8)
                if let Ok(s) = core::str::from_utf8(buf_slice) {
                    serial_write(s);
                    Ok(count as u64)
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            } else { // File descriptor
                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &mut *FILESYSTEM;
                        let result: Result<usize, crate::filesystem::FsError> = fs.write(fd as u32, buf_slice);
                        match result {
                            Ok(_) => Ok(count as u64),
                            Err(e) => Err(fs_error_to_syscall_error(e)),
                        }
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
        }
        x if x == Syscall::Open as u64 => {
            // open(path, flags, mode)
            let path_ptr = arg1 as *const u8;
            let flags = arg2 as u32;
            let _mode = arg3 as u32;

            // Safety: Trust userland pointer for now
            let path_cstr = unsafe { core::ffi::CStr::from_ptr(path_ptr as *const core::ffi::c_char) };
            let path_str = path_cstr.to_str().map_err(|_| SyscallError::InvalidArgument)?;

            let open_flags = OpenFlags::from_bits(flags).ok_or(SyscallError::InvalidArgument)?;

            unsafe {
                if !FILESYSTEM.is_null() {
                    let fs = &mut *FILESYSTEM;
                    let result: Result<FileDescriptor, crate::filesystem::FsError> = fs.open(path_str, open_flags);
                    match result {
                        Ok(fd) => Ok(fd as u64),
                        Err(e) => Err(fs_error_to_syscall_error(e)),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
        }
        x if x == Syscall::Close as u64 => {
            // close(fd)
            let fd = arg1 as u32;

            unsafe {
                if !FILESYSTEM.is_null() {
                    let fs = &mut *FILESYSTEM;
                    let result: Result<(), crate::filesystem::FsError> = fs.close(fd);
                    match result {
                        Ok(()) => Ok(0),
                        Err(e) => Err(fs_error_to_syscall_error(e)),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
        }
        x if x == Syscall::Read as u64 => {
            // read(fd, buf, count)
            let fd = arg1 as u32;
            let buf_ptr = arg2 as *mut u8;
            let count = arg3 as usize;

            unsafe {
                if !FILESYSTEM.is_null() {
                    let fs = &mut *FILESYSTEM;
                    // Use a fixed-size buffer for now
                    let mut buffer = [0u8; 4096];
                    let read_size = core::cmp::min(count, buffer.len());
                    let result: Result<usize, crate::filesystem::FsError> = fs.read(fd, &mut buffer[..read_size]);
                    match result {
                        Ok(bytes_read) => {
                            // Copy back to user buffer
                            core::ptr::copy_nonoverlapping(buffer.as_ptr(), buf_ptr, bytes_read);
                            Ok(bytes_read as u64)
                        }
                        Err(e) => Err(fs_error_to_syscall_error(e)),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
        }
        x if x == Syscall::CreateSnapshot as u64 => {
            // create_snapshot()
            unsafe {
                if !FILESYSTEM.is_null() {
                    let fs = &mut *FILESYSTEM;
                    let result: Result<InodeNum, crate::filesystem::FsError> = fs.create_snapshot();
                    match result {
                        Ok(snapshot_id) => Ok(snapshot_id as u64),
                        Err(e) => Err(fs_error_to_syscall_error(e)),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
        }
        x if x == Syscall::SecurityCheck as u64 => {
            // security_check(operation, level)
            let operation = arg1 as u32;
            let level = arg2 as u32;

            if let Some(sm) = crate::security::get_security_manager() {
                let op_type = match operation {
                    0 => crate::security::OperationType::ModelExecution,
                    1 => crate::security::OperationType::CloudAccess,
                    2 => crate::security::OperationType::KernelPatch,
                    3 => crate::security::OperationType::DriverUpdate,
                    4 => crate::security::OperationType::ModelHotpatch,
                    5 => crate::security::OperationType::DataExport,
                    6 => crate::security::OperationType::SecurityPolicyChange,
                    7 => crate::security::OperationType::AutonomyControl,
                    _ => return Err(SyscallError::InvalidArgument),
                };

                let sec_level = match level {
                    0 => crate::security::SecurityLevel::Low,
                    1 => crate::security::SecurityLevel::Medium,
                    2 => crate::security::SecurityLevel::High,
                    3 => crate::security::SecurityLevel::Critical,
                    _ => return Err(SyscallError::InvalidArgument),
                };

                match sm.check_operation(op_type, sec_level) {
                    Ok(allowed) => Ok(if allowed { 1 } else { 0 }),
                    Err(_) => Ok(0), // Operation not allowed
                }
            } else {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::AuditLog as u64 => {
            // audit_log(operation, success, details_ptr)
            let operation = arg1 as u32;
            let success = arg2 != 0;
            let details_ptr = arg3 as *const u8;

            if let Some(sm) = crate::security::get_security_manager() {
                let op_type = match operation {
                    0 => crate::security::OperationType::ModelExecution,
                    1 => crate::security::OperationType::CloudAccess,
                    2 => crate::security::OperationType::KernelPatch,
                    3 => crate::security::OperationType::DriverUpdate,
                    4 => crate::security::OperationType::ModelHotpatch,
                    5 => crate::security::OperationType::DataExport,
                    6 => crate::security::OperationType::SecurityPolicyChange,
                    7 => crate::security::OperationType::AutonomyControl,
                    _ => return Err(SyscallError::InvalidArgument),
                };

                // Safety: Trust userland pointer for now
                let details_cstr = unsafe { core::ffi::CStr::from_ptr(details_ptr as *const core::ffi::c_char) };
                let details = details_cstr.to_str().map_err(|_| SyscallError::InvalidArgument)?;

                let details_bytes = details.as_bytes();
                let mut details_fixed = [0u8; 256];
                let copy_len = core::cmp::min(details_bytes.len(), details_fixed.len());
                details_fixed[..copy_len].copy_from_slice(&details_bytes[..copy_len]);

                match sm.audit_log(op_type, 0, success, &details_fixed[..copy_len]) {
                    Ok(()) => Ok(0),
                    Err(_) => Err(SyscallError::InvalidArgument),
                }
            } else {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::RedactPII as u64 => {
            // redact_pii(buf, len)
            let buf_ptr = arg1 as *mut u8;
            let len = arg2 as usize;

            if let Some(sm) = crate::security::get_security_manager() {
                // Safety: Trust userland pointer for now
                let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
                let redacted_count = sm.redact_pii(buf_slice);
                Ok(redacted_count as u64)
            } else {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::SetAutonomy as u64 => {
            // set_autonomy(enabled)
            let enabled = arg1 != 0;

            if let Some(sm) = crate::security::get_security_manager() {
                match sm.set_autonomy(enabled, 0) {
                    Ok(()) => Ok(0),
                    Err(_) => Err(SyscallError::PermissionDenied),
                }
            } else {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::KillSwitch as u64 => {
            // kill_switch()
            if let Some(sm) = crate::security::get_security_manager() {
                match sm.kill_switch(0) {
                    Ok(()) => Ok(0),
                    Err(_) => Err(SyscallError::PermissionDenied),
                }
            } else {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::Yield as u64 => {
            // yield()
            crate::scheduler::yield_current();
            Ok(0)
        }
        x if x == Syscall::Sleep as u64 => {
            // sleep(ticks)
            let ticks = arg1 as u32;
            crate::scheduler::sleep_current(ticks);
            Ok(0)
        }
        x if x == Syscall::GetPid as u64 => {
            // getpid()
            if let Some(scheduler) = crate::scheduler::get_scheduler().lock().as_ref() {
                if let Some(pcb) = scheduler.current_process() {
                    Ok(pcb.process.pid as u64)
                } else {
                    Ok(0) // Kernel process
                }
            } else {
                Ok(0)
            }
        }
        _ => Err(SyscallError::InvalidSyscall),
    }
}

/// Syscall interrupt handler
/// This is called when userland executes int 0x80
pub extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    // Syscall number is in RAX
    let syscall_num: u64;
    unsafe {
        core::arch::asm!("mov {}, rax", out(reg) syscall_num);
    }

    // Arguments are in RDI, RSI, RDX, R10, R8, R9 (System V ABI)
    let arg1: u64;
    let arg2: u64;
    let arg3: u64;
    let arg4: u64;
    let arg5: u64;
    let arg6: u64;

    unsafe {
        core::arch::asm!(
            "mov {}, rdi",
            "mov {}, rsi",
            "mov {}, rdx",
            "mov {}, r10",
            "mov {}, r8",
            "mov {}, r9",
            out(reg) arg1,
            out(reg) arg2,
            out(reg) arg3,
            out(reg) arg4,
            out(reg) arg5,
            out(reg) arg6,
        );
    }

    // Handle the syscall
    let result = unsafe { handle_syscall(syscall_num, arg1, arg2, arg3, arg4, arg5, arg6) };

    // Return result in RAX
    let return_value = match result {
        Ok(val) => val,
        Err(err) => err as i64 as u64, // Negative error codes
    };

    unsafe {
        core::arch::asm!("mov rax, {}", in(reg) return_value);
    }

    // Return to userland
}