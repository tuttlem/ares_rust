#![allow(dead_code)]

use crate::drivers::{console, keyboard, CharDevice, DriverError};
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
const KERNEL_STACK_SIZE: usize = 16 * 1024;

type ProcessEntry = extern "C" fn() -> !;

#[repr(C)]
pub struct Context {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl Context {
    const fn new() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rsp: 0,
            rip: 0,
            rflags: 0x202, // IF set
        }
    }
}

extern "C" {
    fn context_switch(current: *mut Context, next: *const Context);
}

extern "C" fn process_exit() -> ! {
    klog!("[process] process exited unexpectedly\n");
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)); }
    }
}

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
    Ready,
    Running,
    Blocked,
    Zombie,
}

pub struct Process {
    pid: Pid,
    parent: Option<Pid>,
    name: &'static str,
    state: ProcessState,
    fds: [Option<FileDescriptor>; MAX_FDS],
    context: Context,
    stack_ptr: *mut u8,
    stack_layout: Option<Layout>,
}

impl Process {
    fn new_kernel(pid: Pid, name: &'static str, parent: Option<Pid>, entry: ProcessEntry) -> Result<Self, ProcessError> {
        let layout = Layout::from_size_align(KERNEL_STACK_SIZE, 16).map_err(|_| ProcessError::StackAllocationFailed)?;
        let stack_ptr = unsafe { heap::allocate(layout) };
        if stack_ptr.is_null() {
            return Err(ProcessError::StackAllocationFailed);
        }

        let stack_top = unsafe { stack_ptr.add(KERNEL_STACK_SIZE) } as u64;
        let mut aligned_top = stack_top & !0xFu64;

        unsafe {
            aligned_top = aligned_top.saturating_sub(8);
            (aligned_top as *mut u64).write(process_exit as u64);
        }

        let mut context = Context::new();
        context.rsp = aligned_top;
        context.rbp = aligned_top;
        context.rip = entry as u64;

        let mut process = Self {
            pid,
            parent,
            name,
            state: ProcessState::Ready,
            fds: [None; MAX_FDS],
            context,
            stack_ptr,
            stack_layout: Some(layout),
        };

        let console_device = console::driver();
        process.set_fd(STDOUT_FD, FileDescriptor::Char(console_device))?;
        process.set_fd(STDERR_FD, FileDescriptor::Char(console_device))?;

        let keyboard_device = keyboard::driver();
        process.set_fd(STDIN_FD, FileDescriptor::Char(keyboard_device))?;

        Ok(process)
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

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(layout) = self.stack_layout.take() {
            unsafe {
                heap::deallocate(self.stack_ptr, layout);
            }
        }
    }
}

#[derive(Debug)]
pub enum ProcessError {
    AllocationFailed,
    TooManyProcesses,
    InvalidFileDescriptor,
    ProcessNotFound,
    StackAllocationFailed,
    NotInitialized,
}

struct ProcessTable {
    entries: *mut Process,
    len: usize,
    capacity: usize,
    next_pid: Pid,
    init_pid: Option<Pid>,
    initialized: bool,
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
            initialized: false,
        }
    }

    fn spawn_kernel_process(&mut self, name: &'static str, parent: Option<Pid>, entry: ProcessEntry) -> Result<Pid, ProcessError> {
        let pid = self.allocate_pid()?;
        let process = Process::new_kernel(pid, name, parent, entry)?;
        self.push(process)?;
        if self.init_pid.is_none() {
            self.init_pid = Some(pid);
        }
        Ok(pid)
    }

    fn allocate_pid(&mut self) -> Result<Pid, ProcessError> {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.checked_add(1).ok_or(ProcessError::TooManyProcesses)?;
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
        let required = self.len.checked_add(additional).ok_or(ProcessError::TooManyProcesses)?;
        if required <= self.capacity {
            return Ok(());
        }

        debug_assert!(self.entries.is_null() == (self.capacity == 0));

        let mut new_capacity = if self.capacity == 0 { 4 } else { self.capacity };
        while new_capacity < required {
            new_capacity = new_capacity.checked_mul(2).ok_or(ProcessError::TooManyProcesses)?;
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

    fn slice(&self) -> &[Process] {
        unsafe { slice::from_raw_parts(self.entries, self.len) }
    }

    fn slice_mut(&mut self) -> &mut [Process] {
        unsafe { slice::from_raw_parts_mut(self.entries, self.len) }
    }

    fn find_index_by_pid(&self, pid: Pid) -> Option<usize> {
        self.slice().iter().position(|p| p.pid == pid)
    }

    fn next_ready_index(&self, start: Option<usize>) -> Option<usize> {
        if self.len == 0 {
            return None;
        }

        let slice = self.slice();
        let mut index = start.map(|i| (i + 1) % self.len).unwrap_or(0);
        let mut inspected = 0;
        while inspected < self.len {
            if slice[index].state == ProcessState::Ready {
                return Some(index);
            }
            index = (index + 1) % self.len;
            inspected += 1;
        }
        None
    }

    fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        let idx = self.find_index_by_pid(pid)?;
        Some(&mut self.slice_mut()[idx])
    }

    fn get(&self, pid: Pid) -> Option<&Process> {
        let idx = self.find_index_by_pid(pid)?;
        Some(&self.slice()[idx])
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
static mut BOOT_CONTEXT: Context = Context::new();

pub fn init() -> Result<(), ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if table.initialized {
        return Ok(());
    }
    table.initialized = true;
    klog!("[process] table initialised\n");
    Ok(())
}

pub fn spawn_kernel_process(name: &'static str, entry: ProcessEntry) -> Result<Pid, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if !table.initialized {
        return Err(ProcessError::NotInitialized);
    }
    let pid = table.spawn_kernel_process(name, current_pid(), entry)?;
    klog!("[process] spawned '{}' pid={}\n", name, pid);
    Ok(pid)
}

pub fn start_scheduler() -> ! {
    loop {
        if !schedule_internal() {
            core::hint::spin_loop();
        }
    }
}

pub fn yield_now() {
    let _ = schedule_internal();
}

fn schedule_internal() -> bool {
    let (current_ctx, next_ctx, next_pid) = {
        let mut table = PROCESS_TABLE.lock();
        if table.len == 0 {
            return false;
        }

        let current_pid = current_pid();
        let current_index = current_pid.and_then(|pid| table.find_index_by_pid(pid));

        let next_index = match table.next_ready_index(current_index) {
            Some(idx) => idx,
            None => return false,
        };

        if let Some(idx) = current_index {
            if idx == next_index {
                return false;
            }
        }

        let slice = table.slice_mut();

        if let Some(idx) = current_index {
            if let Some(process) = slice.get_mut(idx) {
                if process.state == ProcessState::Running {
                    process.state = ProcessState::Ready;
                }
            }
        }

        if let Some(process) = slice.get_mut(next_index) {
            process.state = ProcessState::Running;
        }

        let next_pid = slice[next_index].pid;
        let next_ctx_ptr: *const Context = &slice[next_index].context;

        let current_ctx_ptr: *mut Context = match current_index {
            Some(idx) => &mut slice[idx].context as *mut Context,
            None => unsafe { ptr::addr_of_mut!(BOOT_CONTEXT) },
        };

        (current_ctx_ptr, next_ctx_ptr, next_pid)
    };

    set_current_pid(next_pid);
    unsafe {
        context_switch(current_ctx, next_ctx);
    }
    true
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

pub fn descriptor(pid: Pid, fd: usize) -> Option<FileDescriptor> {
    let table = PROCESS_TABLE.lock();
    table.get(pid).and_then(|process| process.fd(fd))
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

fn state_name(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::Blocked => "Blocked",
        ProcessState::Zombie => "Zombie",
    }
}

pub fn dump_process(pid: Pid) -> Result<(), ProcessError> {
    let table = PROCESS_TABLE.lock();
    let process = table.get(pid).ok_or(ProcessError::ProcessNotFound)?;
    dump_process_inner(process);
    Ok(())
}

pub fn dump_current_process() -> Result<(), ProcessError> {
    if let Some(pid) = current_pid() {
        dump_process(pid)
    } else {
        Err(ProcessError::ProcessNotFound)
    }
}

pub fn dump_all_processes() {
    let table = PROCESS_TABLE.lock();
    let slice = table.slice();
    for process in slice {
        dump_process_inner(process);
    }
}

fn dump_process_inner(process: &Process) {
    klog!("[process] dump pid={} name='{}' state={} parent={:?}\n",
        process.pid,
        process.name,
        state_name(process.state),
        process.parent);

    klog!(
        "           stack_base=0x{:016X} rip=0x{:016X} rsp=0x{:016X} rbp=0x{:016X}\n",
        process.stack_ptr as usize,
        process.context.rip,
        process.context.rsp,
        process.context.rbp
    );
    klog!(
        "           r15=0x{:016X} r14=0x{:016X} r13=0x{:016X} r12=0x{:016X} rbx=0x{:016X}\n",
        process.context.r15,
        process.context.r14,
        process.context.r13,
        process.context.r12,
        process.context.rbx
    );
    klog!("           rflags=0x{:016X}\n", process.context.rflags);

    for (fd, entry) in process.fds.iter().enumerate() {
        if let Some(descriptor) = entry {
            match descriptor {
                FileDescriptor::Char(dev) => {
                    klog!("           fd {:>2}: CharDevice '{}'\n", fd, dev.name());
                }
            }
        }
    }
}
