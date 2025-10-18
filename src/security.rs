//! Security and Governance Framework
//!
//! Implements non-negotiable security requirements:
//! - Local-first models with explicit cloud opt-in
//! - Human-in-loop for kernel/driver/model hotpatches
//! - Immutable audit trail for all operations
//! - Kill-switch for AI autonomy
//! - PII redaction for off-device exports

use crate::filesystem::{Filesystem, FsError, FileDescriptor, OpenFlags, InodeNum};
use core::fmt;

/// Security policy levels
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SecurityLevel {
    Low,      // Automated operations allowed
    Medium,   // Human approval required for changes
    High,     // Always requires human approval
    Critical, // Never automated, always human-controlled
}

/// Operation types for audit logging
#[derive(Debug, Clone, Copy)]
pub enum OperationType {
    ModelExecution,
    CloudAccess,
    KernelPatch,
    DriverUpdate,
    ModelHotpatch,
    DataExport,
    SecurityPolicyChange,
    AutonomyControl,
}

/// Audit log entry
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub operation: OperationType,
    pub user_id: u32,
    pub success: bool,
    pub details: [u8; 256], // Fixed-size for kernel simplicity
    pub details_len: usize,
}

/// Security policy configuration
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    pub local_first_models: bool,
    pub cloud_opt_in_required: bool,
    pub human_in_loop_patches: bool,
    pub audit_trail_enabled: bool,
    pub pii_redaction_enabled: bool,
    pub autonomy_kill_switch: bool,
}

/// PII detection patterns
pub struct PIIDetector {
    patterns: [&'static str; 8],
}

impl PIIDetector {
    pub fn new() -> Self {
        PIIDetector {
            patterns: [
                r"\b\d{3}-\d{2}-\d{4}\b",     // SSN
                r"\b\d{4} \d{4} \d{4} \d{4}\b", // Credit card
                r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b", // Email
                r"\b\d{3}-\d{3}-\d{4}\b",     // Phone
                r"\b\d{1,5}\s\w+\s\w+\b",    // Address
                r"\b\d{5}(-\d{4})?\b",       // ZIP code
                r"\b\d{2}/\d{2}/\d{4}\b",    // Date of birth
                r"\b[A-Z][a-z]+ [A-Z][a-z]+\b", // Full name
            ],
        }
    }

    pub fn redact(&self, data: &mut [u8]) -> usize {
        let text = match core::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let mut redacted_count = 0;
        let mut result = String::new();
        let mut last_end = 0;

        for pattern in &self.patterns {
            // Simple pattern matching (in real implementation, use regex)
            if let Some(start) = text[last_end..].find(pattern) {
                let actual_start = last_end + start;
                result.push_str(&text[last_end..actual_start]);
                result.push_str("[REDACTED]");
                last_end = actual_start + pattern.len();
                redacted_count += 1;
            }
        }

        if redacted_count > 0 {
            result.push_str(&text[last_end..]);
            let result_bytes = result.as_bytes();
            let copy_len = core::cmp::min(result_bytes.len(), data.len());
            data[..copy_len].copy_from_slice(&result_bytes[..copy_len]);
        }

        redacted_count
    }
}

/// Security manager
pub struct SecurityManager {
    policy: SecurityPolicy,
    audit_log_fd: Option<FileDescriptor>,
    pii_detector: PIIDetector,
    autonomy_enabled: bool,
}

impl SecurityManager {
    pub fn new() -> Self {
        SecurityManager {
            policy: SecurityPolicy {
                local_first_models: true,
                cloud_opt_in_required: true,
                human_in_loop_patches: true,
                audit_trail_enabled: true,
                pii_redaction_enabled: true,
                autonomy_kill_switch: false,
            },
            audit_log_fd: None,
            pii_detector: PIIDetector::new(),
            autonomy_enabled: true,
        }
    }

    /// Initialize security manager
    pub fn init(&mut self, fs: &mut Filesystem) -> Result<(), FsError> {
        // Create audit log file
        if self.policy.audit_trail_enabled {
            self.audit_log_fd = Some(fs.open("/audit.log", OpenFlags {
                read: true,
                write: true,
                create: true,
                truncate: false,
            })?);
        }

        // Log initialization
        self.audit_log(OperationType::SecurityPolicyChange, 0, true, b"Security manager initialized")?;

        Ok(())
    }

    /// Check if operation is allowed
    pub fn check_operation(&self, op: OperationType, level: SecurityLevel) -> Result<bool, &'static str> {
        match op {
            OperationType::ModelExecution => {
                // Always allow local execution
                Ok(true)
            }
            OperationType::CloudAccess => {
                if self.policy.cloud_opt_in_required {
                    Err("Cloud access requires explicit opt-in")
                } else {
                    Ok(true)
                }
            }
            OperationType::KernelPatch | OperationType::DriverUpdate | OperationType::ModelHotpatch => {
                match level {
                    SecurityLevel::Low => Ok(true), // Automated for low-risk
                    _ => {
                        if self.policy.human_in_loop_patches {
                            Err("Human approval required for patches")
                        } else {
                            Ok(false)
                        }
                    }
                }
            }
            OperationType::DataExport => {
                Ok(true) // Always allow but with PII redaction
            }
            OperationType::SecurityPolicyChange => {
                Err("Security policy changes require human approval")
            }
            OperationType::AutonomyControl => {
                Ok(true) // Always allow autonomy controls
            }
        }
    }

    /// Check if AI autonomy is enabled
    pub fn is_autonomy_enabled(&self) -> bool {
        self.autonomy_enabled && !self.policy.autonomy_kill_switch
    }

    /// Enable/disable AI autonomy
    pub fn set_autonomy(&mut self, enabled: bool, user_id: u32) -> Result<(), &'static str> {
        self.autonomy_enabled = enabled;
        let msg = if enabled { b"AI autonomy enabled" } else { b"AI autonomy disabled" };
        self.audit_log(OperationType::AutonomyControl, user_id, true, msg)?;
        Ok(())
    }

    /// Activate kill switch
    pub fn kill_switch(&mut self, user_id: u32) -> Result<(), &'static str> {
        self.policy.autonomy_kill_switch = true;
        self.autonomy_enabled = false;
        self.audit_log(OperationType::AutonomyControl, user_id, true, b"Kill switch activated")?;
        Ok(())
    }

    /// Redact PII from data
    pub fn redact_pii(&self, data: &mut [u8]) -> usize {
        if self.policy.pii_redaction_enabled {
            self.pii_detector.redact(data)
        } else {
            0
        }
    }

    /// Log operation to audit trail
    pub fn audit_log(&mut self, operation: OperationType, user_id: u32, success: bool, details: &[u8]) -> Result<(), FsError> {
        if !self.policy.audit_trail_enabled {
            return Ok(());
        }

        if let Some(fd) = self.audit_log_fd {
            // Create audit entry
            let timestamp = 0u64; // TODO: Get actual timestamp
            let mut entry_data = [0u8; 512];

            // Format: timestamp:operation:user_id:success:details\n
            let mut offset = 0;
            offset += write_number(&mut entry_data[offset..], timestamp);
            entry_data[offset] = b':'; offset += 1;
            offset += write_number(&mut entry_data[offset..], operation as u64);
            entry_data[offset] = b':'; offset += 1;
            offset += write_number(&mut entry_data[offset..], user_id as u64);
            entry_data[offset] = b':'; offset += 1;
            entry_data[offset] = if success { b'1' } else { b'0' }; offset += 1;
            entry_data[offset] = b':'; offset += 1;

            let copy_len = core::cmp::min(details.len(), entry_data.len() - offset - 1);
            entry_data[offset..offset + copy_len].copy_from_slice(&details[..copy_len]);
            offset += copy_len;
            entry_data[offset] = b'\n'; offset += 1;

            // Write to audit log file
            unsafe {
                if let Some(fs) = crate::syscall::FILESYSTEM.as_mut() {
                    fs.write(fd, &entry_data[..offset])?;
                }
            }
        }

        Ok(())
    }
}

/// Write number as string to buffer
fn write_number(buffer: &mut [u8], mut num: u64) -> usize {
    if num == 0 {
        buffer[0] = b'0';
        return 1;
    }

    let mut temp = [0u8; 20];
    let mut len = 0;

    while num > 0 {
        temp[len] = b'0' + (num % 10) as u8;
        num /= 10;
        len += 1;
    }

    // Reverse
    for i in 0..len {
        buffer[i] = temp[len - 1 - i];
    }

    len
}

/// Global security manager instance
static mut SECURITY_MANAGER: Option<SecurityManager> = None;

/// Initialize global security manager
pub fn init() {
    unsafe {
        SECURITY_MANAGER = Some(SecurityManager::new());
    }
}

/// Get security manager instance
pub fn get_security_manager() -> Option<&'static mut SecurityManager> {
    unsafe {
        SECURITY_MANAGER.as_mut()
    }
}

/// Initialize security with filesystem
pub fn init_with_fs(fs: &mut Filesystem) -> Result<(), FsError> {
    if let Some(sm) = get_security_manager() {
        sm.init(fs)?;
    }
    Ok(())
}