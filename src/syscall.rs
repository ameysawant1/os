//! Syscall ABI implementation for the UEFI OS kernel
//!
//! System Call Interface
//!
//! Provides a secure interface between user processes and the kernel.

use crate::serial_write;
use x86_64::structures::idt::InterruptStackFrame;
use crate::filesystem::{Filesystem, FileDescriptor, OpenFlags, InodeNum};

/// Global filesystem instance
/// This will be initialized by the kernel during boot
pub static mut FILESYSTEM: *mut Filesystem = core::ptr::null_mut();

/// Global distributed AI coordinator instance
/// This will be initialized by the kernel during boot
#[cfg(feature = "alloc")]
pub static mut DISTRIBUTED_AI: *mut crate::distributed_ai::DistributedAICoordinator = core::ptr::null_mut();

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
    // Distributed AI syscalls
    RegisterAIModel = 13,  // register_ai_model(model_id, model_data, data_len) -> int
    StartFederatedRound = 14, // start_federated_round(model_id, participants, num_participants) -> session_id
    SubmitLocalUpdate = 15,   // submit_local_update(session_id) -> int
    GetAggregatedUpdate = 16, // get_aggregated_update(model_id, round) -> int
    JoinFederatedNetwork = 17, // join_federated_network(node_id) -> int
    LeaveFederatedNetwork = 18, // leave_federated_network() -> int
    // Execution journal replay syscalls
    StartReplay = 19,         // start_replay(snapshot_id) -> int
    StopReplay = 20,          // stop_replay() -> int
    SeekReplay = 21,          // seek_replay(position) -> int
    GetReplayStatus = 22,     // get_replay_status() -> int
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
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> SyscallResult {
    let result = match syscall_num {
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
        x if x == Syscall::RegisterAIModel as u64 => {
            // register_ai_model(model_id, model_data, data_len)
            #[cfg(feature = "alloc")]
            {
                let _model_id = arg1 as u32;
                let model_data_ptr = arg2 as *const u8;
                let data_len = arg3 as usize;

                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let dai = &mut *DISTRIBUTED_AI;

                        // Safety: Trust userland pointer for now
                        let _model_data = core::slice::from_raw_parts(model_data_ptr, data_len);

                        // For now, create a simple text classifier from the data
                        // In a real implementation, this would deserialize the model
                        let model = crate::ai_models::TextClassifier::new(100); // Default max features
                        let _ = dai.register_model(alloc::boxed::Box::new(model));

                        Ok(0)
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::StartFederatedRound as u64 => {
            // start_federated_round(model_id, participants_ptr, num_participants)
            #[cfg(feature = "alloc")]
            {
                let model_id = arg1 as u32;
                let participants_ptr = arg2 as *const u64;
                let num_participants = arg3 as usize;

                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let dai = &mut *DISTRIBUTED_AI;

                        // Safety: Trust userland pointer for now
                        let participants_slice = core::slice::from_raw_parts(participants_ptr, num_participants);
                        let participants: alloc::vec::Vec<crate::distributed_ai::NodeId> =
                            participants_slice.iter().map(|&id| crate::distributed_ai::NodeId(id)).collect();

                        match dai.start_federated_round(model_id, participants) {
                            Ok(session_id) => Ok(session_id),
                            Err(_) => Err(SyscallError::InvalidArgument),
                        }
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::SubmitLocalUpdate as u64 => {
            // submit_local_update(session_id)
            #[cfg(feature = "alloc")]
            {
                let session_id = arg1;

                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let dai = &mut *DISTRIBUTED_AI;

                        match dai.submit_local_update(session_id) {
                            Ok(()) => Ok(0),
                            Err(_) => Err(SyscallError::InvalidArgument),
                        }
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::GetAggregatedUpdate as u64 => {
            // get_aggregated_update(model_id, round)
            #[cfg(feature = "alloc")]
            {
                let _model_id = arg1 as u32;
                let _round = arg2 as u32;

                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let dai = &mut *DISTRIBUTED_AI;

                        // Process any incoming messages first
                        dai.process_incoming_messages().map_err(|_| SyscallError::InvalidArgument)?;

                        // Check if we have an aggregated update for this model and round
                        // This is a simplified implementation - in reality, we'd need to track
                        // which updates have been received and applied
                        Ok(0) // Placeholder - would return 1 if update available
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::JoinFederatedNetwork as u64 => {
            // join_federated_network(node_id)
            #[cfg(feature = "alloc")]
            {
                let _node_id = arg1;

                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let _dai = &mut *DISTRIBUTED_AI;

                        // For now, just acknowledge the join
                        // In a real implementation, this would initiate the join protocol
                        Ok(0)
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::LeaveFederatedNetwork as u64 => {
            // leave_federated_network()
            #[cfg(feature = "alloc")]
            {
                unsafe {
                    if !DISTRIBUTED_AI.is_null() {
                        let _dai = &mut *DISTRIBUTED_AI;

                        // For now, just acknowledge the leave
                        // In a real implementation, this would clean up sessions
                        Ok(0)
                    } else {
                        Err(SyscallError::InvalidArgument)
                    }
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::StartReplay as u64 => {
            // start_replay(snapshot_id)
            let snapshot_id = arg1 as u32;
            #[cfg(feature = "alloc")]
            {
                if let Some(journal) = crate::execution_journal::get_journal() {
                    match journal.start_replay(snapshot_id) {
                        Ok(_) => Ok(0),
                        Err(_) => Err(SyscallError::InvalidArgument),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                let _ = snapshot_id;
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::StopReplay as u64 => {
            // stop_replay()
            #[cfg(feature = "alloc")]
            {
                if let Some(journal) = crate::execution_journal::get_journal() {
                    journal.stop_replay();
                    Ok(0)
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::SeekReplay as u64 => {
            // seek_replay(position)
            let position = arg1 as usize;
            #[cfg(feature = "alloc")]
            {
                if let Some(journal) = crate::execution_journal::get_journal() {
                    match journal.seek_replay(position) {
                        Ok(_) => Ok(0),
                        Err(_) => Err(SyscallError::InvalidArgument),
                    }
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                let _ = position;
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::GetReplayStatus as u64 => {
            // get_replay_status() -> replay_mode (0 = live, 1 = replay)
            #[cfg(feature = "alloc")]
            {
                if let Some(journal) = crate::execution_journal::get_journal() {
                    Ok(if journal.is_replay_mode() { 1 } else { 0 })
                } else {
                    Err(SyscallError::InvalidArgument)
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        _ => Err(SyscallError::InvalidSyscall),
    };

    // Record syscall completion in journal
    #[cfg(feature = "alloc")]
    if let Some(journal) = crate::execution_journal::get_journal() {
        let process_id = 0; // Kernel process for now
        let thread_id = 0;  // Single-threaded for now
        let return_value = match &result {
            Ok(val) => *val,
            Err(err) => *err as i64 as u64, // Error codes are negative i64, store as u64
        };
        let error_code = match &result {
            Ok(_) => 0,
            Err(err) => *err as i64,
        };
        let args = [arg1, arg2, arg3, arg4, arg5, arg6];
        let _ = journal.record_syscall(syscall_num, args, return_value, error_code, process_id, thread_id);
    }

    result
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