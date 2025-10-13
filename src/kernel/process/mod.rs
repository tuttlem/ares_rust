#![allow(dead_code)]

use crate::drivers::console;
use crate::drivers::keyboard;
use crate::drivers::{CharDevice, DriverError};
use crate::klog;
use crate::mem::heap;
use crate::sync::spinlock::SpinLock;

use core::alloc::Layout;
use core::sync::atomic::{AtomicU32, Ordering};
use core::{ptr, slice};

pub type Pid = u32;

pub const STDIN_FD: usize = 0;
pub const STDOUT_FD: usize = 1;
pub const STDERR_FD: usize = 2;
const MAX_FDS: usize = 16;

#[derive(Clone, Copy)]
pub enum FileDescriptor {
    Char(&'static dyn CharDevice),
}

impl FileDescriptor {
    pub fn as_char(&self) -> Option<&'static dyn CharDevice> {
        match self {
            FileDescriptor::Char(device) => Some(*device),
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, DriverError> {
        match self {
            FileDescriptor::Char(device) => device.write(buf),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, DriverError> {
        match self {
            FileDescriptor::Char(device) => device.read(buf),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Kernel,
    Ready,
    Blocked,
    Zombie,
}

pub struct Process {
    pid: Pid,
    parent: Option<Pid>,
    name: &'static str,
    state: ProcessState,
    fds: [Option<FileDescriptor>; MAX_FDS],
}

impl Process {
    fn new_kernel(pid: Pid, name: &'static str, parent: Option<Pid>) -> Self {
        Self {
            pid,
            parent,
            name,
            state: ProcessState::Kernel,
            fds: [None; MAX_FDS],
        }
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn state(&self) -> ProcessState {
        self.state
    }

    pub fn parent(&self) -> Option<Pid> {
        self.parent
    }

    fn set_fd(&mut self, index: usize, descriptor: FileDescriptor) -> Result<(), ProcessError> {
        if index >= MAX_FDS {
            return Err(ProcessError::InvalidFileDescriptor);
        }
        self.fds[index] = Some(descriptor);
        Ok(())
    }

    fn fd(&self, index: usize) -> Option<FileDescriptor> {
        if index >= MAX_FDS {
            return None;
        }
        self.fds[index]
    }
}

#[derive(Debug)]
pub enum ProcessError {
    AllocationFailed,
    TooManyProcesses,
    InvalidFileDescriptor,
    ProcessNotFound,
    AlreadyInitialized,
}

struct ProcessTable {
    entries: *mut Process,
    len: usize,
    capacity: usize,
    next_pid: Pid,
    init_pid: Option<Pid>,
}

unsafe impl Send for ProcessTable {}

impl ProcessTable {
    const fn new() -> Self {
        Self {
            entries: ptr::null_mut(),
            len: 0,
            capacity: 0,
            next_pid: 1,
            init_pid: None,
        }
    }

    fn is_initialized(&self) -> bool {
        self.init_pid.is_some()
    }

    fn init_processes(&mut self) -> Result<Pid, ProcessError> {
        if self.is_initialized() {
            return Err(ProcessError::AlreadyInitialized);
        }

        let pid = self.create_kernel_process("init", None)?;
        self.init_pid = Some(pid);
        Ok(pid)
    }

    fn create_kernel_process(&mut self, name: &'static str, parent: Option<Pid>) -> Result<Pid, ProcessError> {
        let pid = self.allocate_pid()?;
        let mut process = Process::new_kernel(pid, name, parent);

        let console_device = console::driver();
        process.set_fd(STDOUT_FD, FileDescriptor::Char(console_device))?;
        process.set_fd(STDERR_FD, FileDescriptor::Char(console_device))?;

        let keyboard_device = keyboard::driver();
        process.set_fd(STDIN_FD, FileDescriptor::Char(keyboard_device))?;

        self.push(process)?;
        Ok(pid)
    }

    fn allocate_pid(&mut self) -> Result<Pid, ProcessError> {
        let pid = self.next_pid;
        self.next_pid = self
            .next_pid
            .checked_add(1)
            .ok_or(ProcessError::TooManyProcesses)?;
        Ok(pid)
    }

    fn push(&mut self, process: Process) -> Result<(), ProcessError> {
        self.ensure_capacity(1)?;
        unsafe {
            self.entries.add(self.len).write(process);
        }
        self.len += 1;
        Ok(())
    }

    fn ensure_capacity(&mut self, additional: usize) -> Result<(), ProcessError> {
        let required = self
            .len
            .checked_add(additional)
            .ok_or(ProcessError::TooManyProcesses)?;
        if required <= self.capacity {
            return Ok(());
        }

        debug_assert!(self.entries.is_null() == (self.capacity == 0));

        let mut new_capacity = if self.capacity == 0 { 4 } else { self.capacity };
        while new_capacity < required {
            new_capacity = new_capacity
                .checked_mul(2)
                .ok_or(ProcessError::TooManyProcesses)?;
        }

        let layout = Layout::array::<Process>(new_capacity).map_err(|_| ProcessError::AllocationFailed)?;
        let new_ptr = unsafe { heap::allocate(layout) } as *mut Process;
        if new_ptr.is_null() {
            return Err(ProcessError::AllocationFailed);
        }

        unsafe {
            if self.len > 0 {
                ptr::copy_nonoverlapping(self.entries, new_ptr, self.len);
            }
        }

        if self.capacity != 0 {
            let old_layout = Layout::array::<Process>(self.capacity).map_err(|_| ProcessError::AllocationFailed)?;
            unsafe {
                heap::deallocate(self.entries as *mut u8, old_layout);
            }
        }

        self.entries = new_ptr;
        self.capacity = new_capacity;
        Ok(())
    }

    fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        if self.len == 0 {
            return None;
        }
        let slice: &mut [Process] = unsafe { slice::from_raw_parts_mut(self.entries, self.len) };
        slice.iter_mut().find(|process| process.pid == pid)
    }

    fn get(&self, pid: Pid) -> Option<&Process> {
        if self.len == 0 {
            return None;
        }
        let slice: &[Process] = unsafe { slice::from_raw_parts(self.entries, self.len) };
        slice.iter().find(|process| process.pid == pid)
    }
}

impl Drop for ProcessTable {
    fn drop(&mut self) {
        if self.capacity == 0 || self.entries.is_null() {
            return;
        }

        unsafe {
            if self.len > 0 {
                let slice = slice::from_raw_parts_mut(self.entries, self.len);
                for process in slice {
                    ptr::drop_in_place(process);
                }
            }
            if let Ok(layout) = Layout::array::<Process>(self.capacity) {
                heap::deallocate(self.entries as *mut u8, layout);
            }
        }

        self.entries = ptr::null_mut();
        self.len = 0;
        self.capacity = 0;
    }
}

static PROCESS_TABLE: SpinLock<ProcessTable> = SpinLock::new(ProcessTable::new());
static CURRENT_PID: AtomicU32 = AtomicU32::new(0);

pub fn init() -> Result<Pid, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let pid = table.init_processes()?;
    set_current_pid(pid);
    klog!("[process] created init process pid={}\n", pid);
    Ok(pid)
}

pub fn get_process(pid: Pid) -> Option<ProcessSnapshot> {
    let table = PROCESS_TABLE.lock();
    table.get(pid).map(ProcessSnapshot::from)
}

pub struct ProcessSnapshot {
    pid: Pid,
    parent: Option<Pid>,
    name: &'static str,
    state: ProcessState,
}

impl ProcessSnapshot {
    fn from(process: &Process) -> Self {
        Self {
            pid: process.pid,
            parent: process.parent,
            name: process.name,
            state: process.state,
        }
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn parent(&self) -> Option<Pid> {
        self.parent
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn state(&self) -> ProcessState {
        self.state
    }
}

pub fn init_pid() -> Option<Pid> {
    let table = PROCESS_TABLE.lock();
    table.init_pid
}

pub fn current_pid() -> Option<Pid> {
    match CURRENT_PID.load(Ordering::Acquire) {
        0 => None,
        pid => Some(pid as Pid),
    }
}

pub fn set_current_pid(pid: Pid) {
    CURRENT_PID.store(pid, Ordering::Release);
}

pub fn with_process_mut<F, R>(pid: Pid, f: F) -> Result<R, ProcessError>
where
    F: FnOnce(&mut Process) -> R,
{
    let mut table = PROCESS_TABLE.lock();
    let process = table
        .get_mut(pid)
        .ok_or(ProcessError::ProcessNotFound)?;
    Ok(f(process))
}

pub fn descriptor(pid: Pid, fd: usize) -> Option<FileDescriptor> {
    let table = PROCESS_TABLE.lock();
    table.get(pid).and_then(|process| process.fd(fd))
}
