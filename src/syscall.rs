//! Syscall ABI implementation for the UEFI OS kernel
//!
//! System Call Interface
//!
//! Provides a secure interface between user processes and the kernel.

#![allow(dead_code)]

use crate::utils::serial_write;
use x86_64::structures::idt::InterruptStackFrame;
use crate::filesystem::{Filesystem, FileDescriptor, OpenFlags, InodeNum};

/// Global filesystem instance
/// This will be initialized by the kernel during boot
pub static mut FILESYSTEM: *mut Filesystem = core::ptr::null_mut();

/// Global distributed AI coordinator instance
/// This will be initialized by the kernel during boot
#[cfg(feature = "alloc")]
pub static mut DISTRIBUTED_AI: *mut crate::distributed_ai::DistributedAICoordinator = core::ptr::null_mut();

/// Global semantic filesystem instance
/// This will be initialized by the kernel during boot
#[cfg(feature = "alloc")]
pub static mut SEMANTIC_FS: *mut crate::semantic_fs::SemanticFilesystem = core::ptr::null_mut();

/// Global execution journal instance
/// This will be initialized by the kernel during boot
#[cfg(feature = "alloc")]
pub static mut EXECUTION_JOURNAL: *mut crate::execution_journal::ExecutionJournal = core::ptr::null_mut();

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
    RecordContextSwitch = 26,    // record_context_switch(from_process, to_process, from_thread, to_thread, reason) -> int
    RecordAIDecision = 27,       // record_ai_decision(model_id, decision_type, input_hash_ptr, output_hash_ptr, confidence, process_id) -> int
    RecordMemoryAccess = 28,     // record_memory_access(address, size, operation, process_id, thread_id) -> int
    RecordNetworkEvent = 29,     // record_network_event(event_type, source_ip_ptr, dest_ip_ptr, source_port, dest_port, payload_hash_ptr) -> int
    RecordSecurityEvent = 30,    // record_security_event(event_type, user_id, resource_ptr, action_ptr, result) -> int
    CreateJournalSnapshot = 31,  // create_journal_snapshot(reason) -> snapshot_id
    // Semantic filesystem syscalls
    SemanticSearch = 23,      // semantic_search(query, top_k, results_ptr) -> int
    SemanticIndex = 24,       // semantic_index(file_id) -> int
    SemanticGetRecord = 25,   // semantic_get_record(record_id, record_ptr) -> int
    // ML model management syscalls
    RegisterMLModel = 32,     // register_ml_model(model_type, model_data_ptr, data_len) -> model_id
    UnregisterMLModel = 33,   // unregister_ml_model(model_id) -> int
    ListMLModels = 34,        // list_ml_models(models_ptr, max_models) -> int
    GetMLModelStats = 35,     // get_ml_model_stats(stats_ptr) -> int
    // Relationship querying syscalls
    FindRelatedFiles = 36,    // find_related_files(inum, rel_types_ptr, num_types, min_confidence, results_ptr, max_results) -> int
    FindPaths = 37,           // find_paths(start_inum, max_depth, rel_types_ptr, num_types, min_confidence, results_ptr, max_results) -> int
    FindRelationshipSequences = 38, // find_relationship_sequences(start_inum, sequence_ptr, seq_len, min_confidence, results_ptr, max_results) -> int
    FindCycles = 39,          // find_cycles(max_cycle_length, results_ptr, max_results) -> int
    GetRelationshipStats = 40, // get_relationship_stats(stats_ptr) -> int
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
        crate::filesystem::FsError::FileCorrupted => SyscallError::InvalidArgument,
        crate::filesystem::FsError::AnalysisFailed => SyscallError::InvalidArgument,
        crate::filesystem::FsError::InvalidEmbedding => SyscallError::InvalidArgument,
        crate::filesystem::FsError::ProvenanceError => SyscallError::InvalidArgument,
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
        x if x == Syscall::RecordContextSwitch as u64 => {
            // record_context_switch(from_process, to_process, from_thread, to_thread, reason)
            #[cfg(feature = "alloc")]
            {
                let from_process = arg1 as u32;
                let to_process = arg2 as u32;
                let from_thread = arg3 as u32;
                let to_thread = arg4 as u32;
                let reason = arg5 as u8;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;
                        let context_reason = match reason {
                            1 => crate::execution_journal::ContextSwitchReason::TimeSliceExpired,
                            2 => crate::execution_journal::ContextSwitchReason::Yield,
                            3 => crate::execution_journal::ContextSwitchReason::Sleep,
                            4 => crate::execution_journal::ContextSwitchReason::WaitForIO,
                            5 => crate::execution_journal::ContextSwitchReason::Preempted,
                            6 => crate::execution_journal::ContextSwitchReason::Terminated,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.record_context_switch(from_process, to_process, from_thread, to_thread, context_reason) {
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
        x if x == Syscall::RecordAIDecision as u64 => {
            // record_ai_decision(model_id, decision_type, input_hash_ptr, output_hash_ptr, confidence, process_id)
            #[cfg(feature = "alloc")]
            {
                let model_id = arg1 as u32;
                let decision_type = arg2 as u8;
                let input_hash_ptr = arg3 as *const u8;
                let output_hash_ptr = arg4 as *const u8;
                let confidence_bits = arg5 as u32;
                let process_id = arg6 as u32;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;

                        // Safety: Trust userland pointer for now
                        let input_hash_slice = core::slice::from_raw_parts(input_hash_ptr, 32);
                        let output_hash_slice = core::slice::from_raw_parts(output_hash_ptr, 32);

                        let mut input_hash = [0u8; 32];
                        let mut output_hash = [0u8; 32];
                        input_hash.copy_from_slice(input_hash_slice);
                        output_hash.copy_from_slice(output_hash_slice);

                        let confidence = f32::from_bits(confidence_bits);

                        let ai_decision_type = match decision_type {
                            1 => crate::execution_journal::AIDecisionType::Classification,
                            2 => crate::execution_journal::AIDecisionType::Regression,
                            3 => crate::execution_journal::AIDecisionType::Generation,
                            4 => crate::execution_journal::AIDecisionType::FederatedUpdate,
                            5 => crate::execution_journal::AIDecisionType::SecurityAssessment,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.record_ai_decision(model_id, ai_decision_type, input_hash, output_hash, confidence, process_id) {
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
        x if x == Syscall::RecordMemoryAccess as u64 => {
            // record_memory_access(address, size, operation, process_id, thread_id)
            #[cfg(feature = "alloc")]
            {
                let address = arg1;
                let size = arg2 as u32;
                let operation = arg3 as u8;
                let process_id = arg4 as u32;
                let thread_id = arg5 as u32;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;

                        let mem_operation = match operation {
                            1 => crate::execution_journal::MemoryOperation::Read,
                            2 => crate::execution_journal::MemoryOperation::Write,
                            3 => crate::execution_journal::MemoryOperation::Execute,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.record_memory_access(address, size, mem_operation, process_id, thread_id) {
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
        x if x == Syscall::RecordNetworkEvent as u64 => {
            // record_network_event(event_type, source_ip_ptr, dest_ip_ptr, source_port, dest_port, payload_hash_ptr)
            #[cfg(feature = "alloc")]
            {
                let event_type = arg1 as u8;
                let source_ip_ptr = arg2 as *const u8;
                let dest_ip_ptr = arg3 as *const u8;
                let source_port = arg4 as u16;
                let dest_port = arg5 as u16;
                let payload_hash_ptr = arg6 as *const u8;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;

                        // Safety: Trust userland pointer for now
                        let source_ip_slice = core::slice::from_raw_parts(source_ip_ptr, 16);
                        let dest_ip_slice = core::slice::from_raw_parts(dest_ip_ptr, 16);
                        let payload_hash_slice = core::slice::from_raw_parts(payload_hash_ptr, 32);

                        let mut source_ip = [0u8; 16];
                        let mut dest_ip = [0u8; 16];
                        let mut payload_hash = [0u8; 32];
                        source_ip.copy_from_slice(source_ip_slice);
                        dest_ip.copy_from_slice(dest_ip_slice);
                        payload_hash.copy_from_slice(payload_hash_slice);

                        let net_event_type = match event_type {
                            1 => crate::execution_journal::NetworkEventType::Connect,
                            2 => crate::execution_journal::NetworkEventType::Accept,
                            3 => crate::execution_journal::NetworkEventType::Send,
                            4 => crate::execution_journal::NetworkEventType::Receive,
                            5 => crate::execution_journal::NetworkEventType::Close,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.record_network_event(net_event_type, source_ip, dest_ip, source_port, dest_port, payload_hash) {
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
        x if x == Syscall::RecordSecurityEvent as u64 => {
            // record_security_event(event_type, user_id, resource_ptr, action_ptr, result)
            #[cfg(feature = "alloc")]
            {
                let event_type = arg1 as u8;
                let user_id = arg2 as u32;
                let resource_ptr = arg3 as *const u8;
                let action_ptr = arg4 as *const u8;
                let result = arg5 as u8;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;

                        // Safety: Trust userland pointer for now
                        let resource_cstr = core::ffi::CStr::from_ptr(resource_ptr as *const core::ffi::c_char);
                        let action_cstr = core::ffi::CStr::from_ptr(action_ptr as *const core::ffi::c_char);

                        let resource = resource_cstr.to_bytes();
                        let action = action_cstr.to_bytes();

                        let sec_event_type = match event_type {
                            1 => crate::execution_journal::SecurityEventType::Authentication,
                            2 => crate::execution_journal::SecurityEventType::Authorization,
                            3 => crate::execution_journal::SecurityEventType::Audit,
                            4 => crate::execution_journal::SecurityEventType::PolicyViolation,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        let sec_result = match result {
                            1 => crate::execution_journal::SecurityResult::Success,
                            2 => crate::execution_journal::SecurityResult::Failure,
                            3 => crate::execution_journal::SecurityResult::Denied,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.record_security_event(sec_event_type, user_id, resource, action, sec_result) {
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
        x if x == Syscall::CreateJournalSnapshot as u64 => {
            // create_journal_snapshot(reason) -> snapshot_id
            #[cfg(feature = "alloc")]
            {
                let reason = arg1 as u8;

                unsafe {
                    if !EXECUTION_JOURNAL.is_null() {
                        let journal = &mut *EXECUTION_JOURNAL;

                        let snapshot_reason = match reason {
                            1 => crate::execution_journal::SnapshotReason::Periodic,
                            2 => crate::execution_journal::SnapshotReason::BeforeCriticalOperation,
                            3 => crate::execution_journal::SnapshotReason::AfterFailure,
                            4 => crate::execution_journal::SnapshotReason::Manual,
                            5 => crate::execution_journal::SnapshotReason::ReplayPoint,
                            _ => return Err(SyscallError::InvalidArgument),
                        };

                        match journal.create_snapshot_marker(snapshot_reason) {
                            Ok(snapshot_id) => Ok(snapshot_id as u64),
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
        x if x == Syscall::SemanticSearch as u64 => {
            // semantic_search(query, top_k, results_ptr)
            #[cfg(feature = "alloc")]
            {
                let query_ptr = arg1 as *const u8;
                let top_k = arg2 as usize;
                let results_ptr = arg3 as *mut u8;

                unsafe {
                    if !SEMANTIC_FS.is_null() {
                        let sfs = &mut *SEMANTIC_FS;

                        // Safety: Trust userland pointer for now
                        let query_cstr = core::ffi::CStr::from_ptr(query_ptr as *const core::ffi::c_char);
                        let query = query_cstr.to_str().map_err(|_| SyscallError::InvalidArgument)?;

                        match sfs.semantic_search(query, top_k) {
                            Ok(results) => {
                                // Serialize results to user buffer (simplified)
                                // In a real implementation, this would be more sophisticated
                                let mut offset = 0;
                                for result in results.iter().take(top_k) {
                                    if offset + 8 + result.snippet.len() + 1 >= 4096 { break; } // Buffer limit

                                    // Write file_id
                                    let file_id_bytes = (result.file_id.0).to_le_bytes();
                                    core::ptr::copy_nonoverlapping(file_id_bytes.as_ptr(), results_ptr.add(offset), 8);
                                    offset += 8;

                                    // Write snippet length and snippet
                                    let snippet_bytes = result.snippet.as_bytes();
                                    let snippet_len = core::cmp::min(snippet_bytes.len(), 255);
                                    *results_ptr.add(offset) = snippet_len as u8;
                                    offset += 1;
                                    core::ptr::copy_nonoverlapping(snippet_bytes.as_ptr(), results_ptr.add(offset), snippet_len);
                                    offset += snippet_len;
                                }
                                Ok(results.len() as u64)
                            }
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
        x if x == Syscall::SemanticIndex as u64 => {
            // semantic_index(file_id)
            #[cfg(feature = "alloc")]
            {
                let file_id = arg1;

                unsafe {
                    if !SEMANTIC_FS.is_null() {
                        let sfs = &mut *SEMANTIC_FS;

                        // For now, create a dummy file and extract metadata
                        // In a real implementation, this would read the actual file content
                        let dummy_content = b"This is sample file content for semantic indexing.";
                        let file_id_struct = crate::semantic_fs::FileId(file_id);

                        match sfs.extract_semantic_metadata(file_id_struct, dummy_content) {
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
        x if x == Syscall::SemanticGetRecord as u64 => {
            // semantic_get_record(record_id, record_ptr)
            #[cfg(feature = "alloc")]
            {
                let record_id = arg1 as u64;
                let record_ptr = arg2 as *mut u8;

                unsafe {
                    if !SEMANTIC_FS.is_null() {
                        let sfs = &*SEMANTIC_FS;

                        // Find the record
                        if let Some(record) = sfs.semantic_records.iter().find(|r| r.record_id == record_id) {
                            // Serialize record to user buffer (simplified)
                            let summary_bytes = record.summary.as_bytes();
                            let summary_len = core::cmp::min(summary_bytes.len(), 255);

                            // Write summary length and summary
                            *record_ptr = summary_len as u8;
                            core::ptr::copy_nonoverlapping(summary_bytes.as_ptr(), record_ptr.add(1), summary_len);

                            Ok(0)
                        } else {
                            Err(SyscallError::InvalidArgument)
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
        x if x == Syscall::RegisterMLModel as u64 => {
            // register_ml_model(model_type, model_data_ptr, data_len)
            #[cfg(feature = "alloc")]
            {
                let model_type = arg1 as u32;
                let _model_data_ptr = arg2 as *const u8;
                let _data_len = arg3 as usize;

                // For now, create built-in models based on type
                let model: alloc::boxed::Box<dyn crate::filesystem::LocalModel> = match model_type {
                    0 => alloc::boxed::Box::new(crate::filesystem::SimpleTextClassifier::new(1000)),
                    1 => alloc::boxed::Box::new(crate::filesystem::SimpleEmbeddingGenerator::new(384)),
                    2 => alloc::boxed::Box::new(crate::filesystem::SimpleEntityRecognizer::new()),
                    3 => alloc::boxed::Box::new(crate::filesystem::SimpleLanguageDetector::new()),
                    _ => return Err(SyscallError::InvalidArgument),
                };

                match crate::filesystem::register_ml_model(model) {
                    Ok(model_id) => Ok(model_id.value() as u64),
                    Err(_) => Err(SyscallError::InvalidArgument),
                }
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::UnregisterMLModel as u64 => {
            // unregister_ml_model(model_id)
            #[cfg(feature = "alloc")]
            {
                let model_id = arg1 as u32;

                let success = crate::filesystem::unregister_ml_model(crate::filesystem::ModelId::new(model_id));
                Ok(if success { 0u64 } else { u64::MAX })
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::ListMLModels as u64 => {
            // list_ml_models(models_ptr, max_models)
            #[cfg(feature = "alloc")]
            {
                let models_ptr = arg1 as *mut u8;
                let max_models = arg2 as usize;

                let models = crate::filesystem::list_ml_models();
                let num_models = core::cmp::min(models.len(), max_models);

                unsafe {
                    for i in 0..num_models {
                        let model = &models[i];
                        let offset = i * 64; // Fixed size per model info

                        // Write model ID (4 bytes)
                        let id_bytes = (model.id.value() as u32).to_le_bytes();
                        core::ptr::copy_nonoverlapping(id_bytes.as_ptr(), models_ptr.add(offset), 4);

                        // Write model name (up to 32 bytes)
                        let name_bytes = model.name.as_bytes();
                        let name_len = core::cmp::min(name_bytes.len(), 31);
                        *models_ptr.add(offset + 4) = name_len as u8;
                        core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), models_ptr.add(offset + 5), name_len);

                        // Write model type (1 byte)
                        *models_ptr.add(offset + 36) = model.model_type as u8;

                        // Write usage count (8 bytes)
                        let usage_bytes = model.usage_count.to_le_bytes();
                        core::ptr::copy_nonoverlapping(usage_bytes.as_ptr(), models_ptr.add(offset + 37), 8);
                    }
                }

                Ok(num_models as u64)
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::GetMLModelStats as u64 => {
            // get_ml_model_stats(stats_ptr)
            #[cfg(feature = "alloc")]
            {
                let stats_ptr = arg1 as *mut u8;

                let stats = crate::filesystem::get_ml_model_stats();

                unsafe {
                    // Write total_models (8 bytes)
                    let total_models_bytes = stats.total_models.to_le_bytes();
                    core::ptr::copy_nonoverlapping(total_models_bytes.as_ptr(), stats_ptr, 8);

                    // Write max_models (8 bytes)
                    let max_models_bytes = stats.max_models.to_le_bytes();
                    core::ptr::copy_nonoverlapping(max_models_bytes.as_ptr(), stats_ptr.add(8), 8);

                    // Write total_usage (8 bytes)
                    let total_usage_bytes = stats.total_usage.to_le_bytes();
                    core::ptr::copy_nonoverlapping(total_usage_bytes.as_ptr(), stats_ptr.add(16), 8);
                }

                Ok(0)
            }
            #[cfg(not(feature = "alloc"))]
            {
                Err(SyscallError::InvalidArgument)
            }
        }
        x if x == Syscall::FindRelatedFiles as u64 => {
            // find_related_files(inum, rel_types_ptr, num_types, min_confidence, results_ptr, max_results)
            #[cfg(feature = "alloc")]
            {
                let inum = arg1 as u32;
                let rel_types_ptr = arg2 as *const u32;
                let num_types = arg3 as usize;
                let min_confidence_bits = arg4 as u32;
                let results_ptr = arg5 as *mut u8;
                let max_results = arg6 as usize;

                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &*FILESYSTEM;

                        // Convert relationship types
                        let rel_types_slice = core::slice::from_raw_parts(rel_types_ptr, num_types);
                        let mut rel_types = alloc::vec::Vec::new();
                        for &rel_type_num in rel_types_slice {
                            if let Some(rel_type) = crate::filesystem::num_to_relationship_type(rel_type_num) {
                                rel_types.push(rel_type);
                            }
                        }

                        let rel_types_filter = if rel_types.is_empty() { None } else { Some(rel_types.as_slice()) };
                        let min_confidence = f32::from_bits(min_confidence_bits);

                        let results = fs.find_related_files_syscall(
                            inum, rel_types_filter, min_confidence
                        );

                        // Serialize results to user buffer
                        let mut offset = 0;
                        let num_results = core::cmp::min(results.len(), max_results);

                        for (_i, (target_inum, rel_type, confidence)) in results.iter().enumerate().take(num_results) {
                            if offset + 4 + 4 + 4 >= 4096 { break; } // Buffer limit

                            // Write target inum (4 bytes)
                            let inum_bytes = target_inum.to_le_bytes();
                            core::ptr::copy_nonoverlapping(inum_bytes.as_ptr(), results_ptr.add(offset), 4);
                            offset += 4;

                            // Write relationship type (4 bytes)
                            let rel_type_bytes = (*rel_type as u32).to_le_bytes();
                            core::ptr::copy_nonoverlapping(rel_type_bytes.as_ptr(), results_ptr.add(offset), 4);
                            offset += 4;

                            // Write confidence (4 bytes)
                            let confidence_bytes = confidence.to_le_bytes();
                            core::ptr::copy_nonoverlapping(confidence_bytes.as_ptr(), results_ptr.add(offset), 4);
                            offset += 4;
                        }

                        Ok(num_results as u64)
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
        x if x == Syscall::FindPaths as u64 => {
            // find_paths(start_inum, max_depth, rel_types_ptr, num_types, min_confidence, results_ptr, max_results)
            #[cfg(feature = "alloc")]
            {
                let start_inum = arg1 as u32;
                let max_depth = arg2 as usize;
                let rel_types_ptr = arg3 as *const u32;
                let num_types = arg4 as usize;
                let min_confidence_bits = arg5 as u32;
                let _results_ptr = arg6 as *mut u8;
                let max_results = arg6 as usize; // Note: reusing arg6, should be arg7 but limited to 6 args

                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &*FILESYSTEM;

                        // Convert relationship types
                        let rel_types_slice = core::slice::from_raw_parts(rel_types_ptr, num_types);
                        let mut rel_types = alloc::vec::Vec::new();
                        for &rel_type_num in rel_types_slice {
                            if let Some(rel_type) = crate::filesystem::num_to_relationship_type(rel_type_num) {
                                rel_types.push(rel_type);
                            }
                        }

                        let rel_types_filter = if rel_types.is_empty() { None } else { Some(rel_types.as_slice()) };
                        let _min_confidence = f32::from_bits(min_confidence_bits);

                        let paths = fs.find_paths_syscall(
                            start_inum, 0, rel_types_filter, max_depth
                        );

                        // Serialize paths to user buffer (simplified - just count for now)
                        // In a real implementation, this would serialize the full path structures
                        Ok(core::cmp::min(paths.len(), max_results) as u64)
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
        x if x == Syscall::FindRelationshipSequences as u64 => {
            // find_relationship_sequences(start_inum, sequence_ptr, seq_len, min_confidence, results_ptr, max_results)
            #[cfg(feature = "alloc")]
            {
                let start_inum = arg1 as u32;
                let sequence_ptr = arg2 as *const u32;
                let seq_len = arg3 as usize;
                let min_confidence_bits = arg4 as u32;
                let _results_ptr = arg5 as *mut u8;
                let _max_results = arg6 as usize;

                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &*FILESYSTEM;

                        // Convert sequence
                        let sequence_slice = core::slice::from_raw_parts(sequence_ptr, seq_len);
                        let mut sequence = alloc::vec::Vec::new();
                        for &rel_type_num in sequence_slice {
                            if let Some(rel_type) = crate::filesystem::num_to_relationship_type(rel_type_num) {
                                sequence.push(rel_type);
                            }
                        }

                        let _min_confidence = f32::from_bits(min_confidence_bits);

                        let results = fs.find_relationship_sequences_syscall(
                            start_inum, &sequence, seq_len
                        );

                        Ok(results.len() as u64)
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
        x if x == Syscall::FindCycles as u64 => {
            // find_cycles(max_cycle_length, results_ptr, max_results)
            #[cfg(feature = "alloc")]
            {
                let max_cycle_length = arg1 as usize;
                let _results_ptr = arg2 as *mut u8;
                let _max_results = arg3 as usize;

                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &*FILESYSTEM;

                        let cycles = fs.find_cycles_syscall(0, max_cycle_length);
                        Ok(cycles.len() as u64)
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
        x if x == Syscall::GetRelationshipStats as u64 => {
            // get_relationship_stats(stats_ptr)
            #[cfg(feature = "alloc")]
            {
                let stats_ptr = arg1 as *mut u8;

                unsafe {
                    if !FILESYSTEM.is_null() {
                        let fs = &*FILESYSTEM;

                        let stats = fs.get_relationship_stats_syscall();

                        // Write stats to user buffer
                        let total_nodes_bytes = stats.total_nodes.to_le_bytes();
                        core::ptr::copy_nonoverlapping(total_nodes_bytes.as_ptr(), stats_ptr, 8);

                        let nodes_with_relationships_bytes = stats.nodes_with_relationships.to_le_bytes();
                        core::ptr::copy_nonoverlapping(nodes_with_relationships_bytes.as_ptr(), stats_ptr.add(8), 8);

                        let total_relationships_bytes = stats.total_relationships.to_le_bytes();
                        core::ptr::copy_nonoverlapping(total_relationships_bytes.as_ptr(), stats_ptr.add(16), 8);

                        let max_relationships_per_file_bytes = stats.max_relationships_per_file.to_le_bytes();
                        core::ptr::copy_nonoverlapping(max_relationships_per_file_bytes.as_ptr(), stats_ptr.add(24), 8);

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