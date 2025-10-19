//! Process Scheduler
//!
//! Implements round-robin scheduling with context switching.
//! Manages process states, time slices, and CPU time allocation.

use alloc::collections::VecDeque;
use crate::process::Process;
use x86_64::structures::idt::InterruptStackFrame;
use spin::Mutex;
use core::arch::asm;

#[cfg(feature = "alloc")]
type ContextSwitchReason = crate::execution_journal::ContextSwitchReason;
#[cfg(not(feature = "alloc"))]
#[derive(Debug, Clone, Copy)]
enum ContextSwitchReason {
    TimeSliceExpired,
    Yield,
    Sleep,
    WaitForIO,
    Terminated,
}

#[cfg(feature = "alloc")]
fn record_context_switch(
    from_process: u32,
    to_process: u32,
    from_thread: u32,
    to_thread: u32,
    reason: ContextSwitchReason,
) {
    if let Some(journal) = crate::execution_journal::get_journal() {
        let _ = journal.record_context_switch(from_process, to_process, from_thread, to_thread, reason);
    }
}

#[cfg(not(feature = "alloc"))]
fn record_context_switch(
    _from_process: u32,
    _to_process: u32,
    _from_thread: u32,
    _to_thread: u32,
    _reason: ContextSwitchReason,
) {
}

/// Process states for scheduling
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchedulerState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// Process control block for scheduling
#[derive(Debug)]
pub struct ProcessControlBlock {
    pub process: Process,
    pub state: SchedulerState,
    pub time_slice: u32,        // Remaining time slice in ticks
    pub total_runtime: u64,     // Total CPU time used
    pub priority: u8,           // 0 = highest, 255 = lowest
}

impl ProcessControlBlock {
    pub fn new(process: Process, priority: u8) -> Self {
        ProcessControlBlock {
            process,
            state: SchedulerState::Ready,
            time_slice: DEFAULT_TIME_SLICE,
            total_runtime: 0,
            priority,
        }
    }
}

/// Round-robin scheduler
pub struct Scheduler {
    processes: VecDeque<ProcessControlBlock>,
    current_process: Option<usize>, // Index in processes queue
    time_slice_counter: u32,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            processes: VecDeque::new(),
            current_process: None,
            time_slice_counter: 0,
        }
    }

    /// Add a process to the scheduler
    pub fn add_process(&mut self, process: Process, priority: u8) {
        let pcb = ProcessControlBlock::new(process, priority);
        self.processes.push_back(pcb);
    }

    /// Schedule next process (called by timer interrupt)
    pub fn schedule(&mut self) -> Option<&mut ProcessControlBlock> {
        self.time_slice_counter += 1;

        let mut from_process = 0;
        let mut to_process = 0;
        let mut from_thread = 0;
        let mut to_thread = 0;
        let mut reason = crate::execution_journal::ContextSwitchReason::TimeSliceExpired;

        // Check if current process needs to be preempted
        let expired = if let Some(idx) = self.current_process {
            let pcb = &self.processes[idx];
            pcb.time_slice == 0 || pcb.state != SchedulerState::Running
        } else {
            true
        };

        if !expired {
            let idx = self.current_process.unwrap();
            let current_pcb = &mut self.processes[idx];
            current_pcb.time_slice = current_pcb.time_slice.saturating_sub(1);
            current_pcb.total_runtime += 1;
            from_process = current_pcb.process.pid;
            from_thread = 0; // Single-threaded for now
            return Some(current_pcb);
        } else {
            // Current process expired or blocked
            if let Some(idx) = self.current_process {
                let current_pcb = &mut self.processes[idx];
                if current_pcb.time_slice == 0 {
                    reason = crate::execution_journal::ContextSwitchReason::TimeSliceExpired;
                } else if current_pcb.state == SchedulerState::Blocked {
                    reason = crate::execution_journal::ContextSwitchReason::WaitForIO;
                } else {
                    reason = crate::execution_journal::ContextSwitchReason::Yield;
                }
                current_pcb.state = SchedulerState::Ready;
                from_process = current_pcb.process.pid;
                from_thread = 0;
            }
            self.current_process = None;
        }

        // Find next ready process
        for (i, pcb) in self.processes.iter_mut().enumerate() {
            if pcb.state == SchedulerState::Ready {
                to_process = pcb.process.pid;
                to_thread = 0; // Single-threaded for now

                pcb.state = SchedulerState::Running;
                pcb.time_slice = DEFAULT_TIME_SLICE;
                self.current_process = Some(i);

                // Record context switch in execution journal
                if let Some(journal) = crate::execution_journal::get_journal() {
                    let _ = journal.record_context_switch(from_process, to_process, from_thread, to_thread, reason);
                }

                return Some(pcb);
            }
        }

        // No ready processes - record that we're switching to idle
        if from_process != 0 {
            // Record switch to idle
            if let Some(journal) = crate::execution_journal::get_journal() {
                let _ = journal.record_context_switch(from_process, 0, from_thread, 0, crate::execution_journal::ContextSwitchReason::Terminated);
            }
        }

        // No ready processes
        self.current_process = None;
        None
    }

    /// Block current process
    pub fn block_current(&mut self) {
        if let Some(idx) = self.current_process {
            if let Some(pcb) = self.processes.get_mut(idx) {
                pcb.state = SchedulerState::Blocked;
            }
            self.current_process = None;
        }
    }

    /// Unblock a process by PID
    pub fn unblock_process(&mut self, pid: u32) {
        for pcb in &mut self.processes {
            if pcb.process.pid == pid && pcb.state == SchedulerState::Blocked {
                pcb.state = SchedulerState::Ready;
                break;
            }
        }
    }

    /// Terminate a process
    pub fn terminate_process(&mut self, pid: u32) {
        if let Some(idx) = self.current_process {
            if self.processes.get(idx).map_or(false, |pcb| pcb.process.pid == pid) {
                self.current_process = None;
            }
        }

        self.processes.retain(|pcb| {
            if pcb.process.pid == pid {
                // TODO: Clean up process resources
                false
            } else {
                true
            }
        });
    }

    /// Get current running process
    pub fn current_process(&self) -> Option<&ProcessControlBlock> {
        self.current_process.and_then(|idx| self.processes.get(idx))
    }

    /// Get process count
    pub fn process_count(&self) -> usize {
        self.processes.len()
    }
}

/// Global scheduler instance
static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

/// Default time slice (in timer ticks)
const DEFAULT_TIME_SLICE: u32 = 10; // ~10ms at 100Hz

/// Initialize the scheduler
pub fn init() {
    *SCHEDULER.lock() = Some(Scheduler::new());
}

/// Get scheduler instance
pub fn get_scheduler() -> &'static Mutex<Option<Scheduler>> {
    &SCHEDULER
}

/// Context switching structure
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cs: u64,
    pub ss: u64,
}

/// Save current context
pub unsafe extern "C" fn save_context(_context: *mut Context) {
    asm!(
        "mov [rdi + 0*8], rax",
        "mov [rdi + 1*8], rbx",
        "mov [rdi + 2*8], rcx",
        "mov [rdi + 3*8], rdx",
        "mov [rdi + 4*8], rsi",
        "mov [rdi + 5*8], rdi", // This will be wrong, need to handle differently
        "mov [rdi + 6*8], rbp",
        "mov [rdi + 7*8], rsp",
        "mov [rdi + 8*8], r8",
        "mov [rdi + 9*8], r9",
        "mov [rdi + 10*8], r10",
        "mov [rdi + 11*8], r11",
        "mov [rdi + 12*8], r12",
        "mov [rdi + 13*8], r13",
        "mov [rdi + 14*8], r14",
        "mov [rdi + 15*8], r15",
        "mov rax, [rsp]",
        "mov [rdi + 16*8], rax", // RIP
        "pushfq",
        "pop rax",
        "mov [rdi + 17*8], rax", // RFLAGS
        "mov ax, cs",
        "mov [rdi + 18*8], rax",
        "mov ax, ss",
        "mov [rdi + 19*8], rax",
        "ret",
        options(noreturn)
    );
}

/// Restore context

pub unsafe extern "C" fn restore_context(_context: *const Context) {
    asm!(
        "mov rax, [rdi + 0*8]",
        "mov rbx, [rdi + 1*8]",
        "mov rcx, [rdi + 2*8]",
        "mov rdx, [rdi + 3*8]",
        "mov rsi, [rdi + 4*8]",
        "mov rbp, [rdi + 6*8]",
        "mov rsp, [rdi + 7*8]",
        "mov r8, [rdi + 8*8]",
        "mov r9, [rdi + 9*8]",
        "mov r10, [rdi + 10*8]",
        "mov r11, [rdi + 11*8]",
        "mov r12, [rdi + 12*8]",
        "mov r13, [rdi + 13*8]",
        "mov r14, [rdi + 14*8]",
        "mov r15, [rdi + 15*8]",
        "push qword ptr [rdi + 17*8]", // RFLAGS
        "popfq",
        "push qword ptr [rdi + 16*8]", // RIP
        "mov ax, [rdi + 18*8]",
        "mov cs, ax",
        "mov ax, [rdi + 19*8]",
        "mov ss, ax",
        "mov rdi, [rdi + 5*8]", // Restore RDI last
        "ret",
        options(noreturn)
    );
}

/// Timer interrupt handler for scheduling
pub extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    if let Some(scheduler) = get_scheduler().lock().as_mut() {
        scheduler.schedule();
    }

    // Send EOI - try APIC first, fall back to PIC
    if let Some(apic) = crate::apic::get_apic() {
        apic.notify_end_of_interrupt(32); // Timer vector
    } else {
        // Fall back to legacy PIC
        unsafe {
            (&mut *core::ptr::addr_of_mut!(crate::PICS)).notify_end_of_interrupt(32);
        }
    }
}

/// Yield current process (cooperative scheduling)
pub fn yield_current() {
    if let Some(scheduler) = get_scheduler().lock().as_mut() {
        scheduler.schedule();
    }
}

/// Sleep current process for specified ticks
pub fn sleep_current(_ticks: u32) {
    // TODO: Implement timer-based wakeups
    if let Some(scheduler) = get_scheduler().lock().as_mut() {
        scheduler.block_current();
        scheduler.schedule();
    }
}

/// Wake up a sleeping process
pub fn wake_process(pid: u32) {
    if let Some(scheduler) = get_scheduler().lock().as_mut() {
        scheduler.unblock_process(pid);
    }
}