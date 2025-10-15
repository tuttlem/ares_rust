#![allow(dead_code)]

use crate::drivers::{console, keyboard, CharDevice, DriverError};
use crate::klog;
use crate::mem::heap;
use crate::sync::spinlock::SpinLock;

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::kernel::interrupts::InterruptFrame;

use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::{ptr, slice};

pub type Pid = u32;

pub const STDIN_FD: usize = 0;
pub const STDOUT_FD: usize = 1;
pub const STDERR_FD: usize = 2;
const MAX_FDS: usize = 16;
const KERNEL_STACK_SIZE: usize = 16 * 1024;

type ProcessEntry = extern "C" fn() -> !;

#[derive(Clone, Copy, Debug)]
pub enum MemoryRegionKind {
    Stack,
    Heap,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryPermissions {
    read: bool,
    write: bool,
    execute: bool,
}

impl MemoryPermissions {
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        Self { read, write, execute }
    }

    pub const fn read_write() -> Self {
        Self::new(true, true, false)
    }

    pub const fn read_only() -> Self {
        Self::new(true, false, false)
    }

    pub const fn read_execute() -> Self {
        Self::new(true, false, true)
    }

    pub fn read(&self) -> bool {
        self.read
    }

    pub fn write(&self) -> bool {
        self.write
    }

    pub fn execute(&self) -> bool {
        self.execute
    }
}

#[derive(Clone, Copy)]
struct MemoryRegion {
    base: *mut u8,
    layout: Layout,
    kind: MemoryRegionKind,
    permissions: MemoryPermissions,
}

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
    fn preempt_trampoline();
}

extern "C" fn process_exit() -> ! {
    klog!("[process] process exited unexpectedly\n");
    exit_current(-1)
}

extern "C" fn idle_task() -> ! {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitChannel {
    KeyboardInput,
    ChildAny,
    Child(Pid),
}

impl WaitChannel {
    fn matches_event(self, event: WaitChannel) -> bool {
        match (self, event) {
            (WaitChannel::KeyboardInput, WaitChannel::KeyboardInput) => true,
            (WaitChannel::ChildAny, WaitChannel::Child(_)) => true,
            (WaitChannel::Child(wait_pid), WaitChannel::Child(event_pid)) => wait_pid == event_pid,
            _ => false,
        }
    }

    fn is_child_event(self) -> bool {
        matches!(self, WaitChannel::Child(_) | WaitChannel::ChildAny)
    }
}

pub struct Process {
    pid: Pid,
    parent: Option<Pid>,
    name: &'static str,
    state: ProcessState,
    wait_channel: Option<WaitChannel>,
    exit_code: Option<i32>,
    is_idle: bool,
    preempt_return: Option<u64>,
    cpu_slices: u64,
    fds: [Option<FileDescriptor>; MAX_FDS],
    context: Context,
    stack_ptr: *mut u8,
    stack_layout: Option<Layout>,
    regions: MemoryRegionList,
}

impl Process {
    fn new_kernel(
        pid: Pid,
        name: &'static str,
        parent: Option<Pid>,
        entry: ProcessEntry,
        is_idle: bool,
    ) -> Result<Self, ProcessError> {
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
            wait_channel: None,
            exit_code: None,
            is_idle,
            preempt_return: None,
            cpu_slices: 0,
            fds: [None; MAX_FDS],
            context,
            stack_ptr,
            stack_layout: Some(layout),
            regions: MemoryRegionList::new(),
        };

        let console_device = console::driver();
        process.set_fd(STDOUT_FD, FileDescriptor::Char(console_device))?;
        process.set_fd(STDERR_FD, FileDescriptor::Char(console_device))?;

        let keyboard_device = keyboard::driver();
        process.set_fd(STDIN_FD, FileDescriptor::Char(keyboard_device))?;

        process.regions.register(MemoryRegion {
            base: stack_ptr,
            layout,
            kind: MemoryRegionKind::Stack,
            permissions: MemoryPermissions::read_write(),
        })?;

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

    pub fn is_idle(&self) -> bool {
        self.is_idle
    }

    pub fn cpu_slices(&self) -> u64 {
        self.cpu_slices
    }

    fn set_preempt_return(&mut self, rip: u64) {
        self.preempt_return = Some(rip);
    }

    fn take_preempt_return(&mut self) -> Option<u64> {
        self.preempt_return.take()
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

    fn allocate_region_with_permissions(
        &mut self,
        layout: Layout,
        kind: MemoryRegionKind,
        permissions: MemoryPermissions,
    ) -> Result<*mut u8, ProcessError> {
        let ptr = unsafe { heap::allocate(layout) };
        if ptr.is_null() {
            return Err(ProcessError::AllocationFailed);
        }
        self.regions.register(MemoryRegion {
            base: ptr,
            layout,
            kind,
            permissions,
        })?;
        Ok(ptr)
    }

    fn allocate_region(&mut self, layout: Layout, kind: MemoryRegionKind) -> Result<*mut u8, ProcessError> {
        self.allocate_region_with_permissions(layout, kind, MemoryPermissions::read_write())
    }

    fn release_region(&mut self, ptr: *mut u8) -> Result<(), ProcessError> {
        if let Some(region) = self.regions.remove_by_ptr(ptr) {
            if !region.base.is_null() {
                unsafe {
                    heap::deallocate(region.base, region.layout);
                }
            }
            Ok(())
        } else {
            Err(ProcessError::MemoryRegionNotFound)
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        for region in self.regions.drain() {
            match region.kind {
                MemoryRegionKind::Stack => {
                    if let Some(layout) = self.stack_layout.take() {
                        unsafe {
                            heap::deallocate(self.stack_ptr, layout);
                        }
                    }
                }
                _ => unsafe {
                    heap::deallocate(region.base, region.layout);
                },
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
    MemoryRegionNotFound,
    IdleAlreadyExists,
    NoChildren,
    ChildNotFound,
}

struct MemoryRegionList {
    regions: *mut MemoryRegion,
    len: usize,
    capacity: usize,
}

impl MemoryRegionList {
    const fn new() -> Self {
        Self {
            regions: ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    fn register(&mut self, region: MemoryRegion) -> Result<(), ProcessError> {
        self.ensure_capacity(1)?;
        unsafe {
            self.regions.add(self.len).write(region);
        }
        self.len += 1;
        Ok(())
    }

    fn remove_by_ptr(&mut self, ptr: *mut u8) -> Option<MemoryRegion> {
        for index in 0..self.len {
            unsafe {
                let entry_ptr = self.regions.add(index);
                if (*entry_ptr).base == ptr {
                    let removed = entry_ptr.read();
                    if index != self.len - 1 {
                        let last = self.regions.add(self.len - 1).read();
                        self.regions.add(index).write(last);
                    }
                    self.len -= 1;
                    return Some(removed);
                }
            }
        }
        None
    }

    fn iter(&self) -> core::slice::Iter<'_, MemoryRegion> {
        self.as_slice().iter()
    }

    fn drain(&mut self) -> DrainMemoryRegions {
        DrainMemoryRegions { list: self }
    }

    fn as_slice(&self) -> &[MemoryRegion] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.regions, self.len) }
        }
    }

    fn as_slice_mut(&mut self) -> &mut [MemoryRegion] {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.regions, self.len) }
        }
    }

    fn ensure_capacity(&mut self, additional: usize) -> Result<(), ProcessError> {
        let required = self.len.checked_add(additional).ok_or(ProcessError::AllocationFailed)?;
        if required <= self.capacity {
            return Ok(());
        }

        let mut new_capacity = if self.capacity == 0 { 4 } else { self.capacity };
        while new_capacity < required {
            new_capacity = new_capacity.checked_mul(2).ok_or(ProcessError::AllocationFailed)?;
        }

        let layout = Layout::array::<MemoryRegion>(new_capacity).map_err(|_| ProcessError::AllocationFailed)?;
        let new_ptr = unsafe { heap::allocate(layout) } as *mut MemoryRegion;
        if new_ptr.is_null() {
            return Err(ProcessError::AllocationFailed);
        }

        unsafe {
            if self.len > 0 {
                ptr::copy_nonoverlapping(self.regions, new_ptr, self.len);
            }
        }

        if self.capacity != 0 {
            let old_layout = Layout::array::<MemoryRegion>(self.capacity).map_err(|_| ProcessError::AllocationFailed)?;
            unsafe {
                heap::deallocate(self.regions as *mut u8, old_layout);
            }
        }

        self.regions = new_ptr;
        self.capacity = new_capacity;
        Ok(())
    }
}

impl Drop for MemoryRegionList {
    fn drop(&mut self) {
        if self.capacity != 0 && !self.regions.is_null() {
            if let Ok(layout) = Layout::array::<MemoryRegion>(self.capacity) {
                unsafe {
                    heap::deallocate(self.regions as *mut u8, layout);
                }
            }
        }
        self.regions = ptr::null_mut();
        self.len = 0;
        self.capacity = 0;
    }
}

struct DrainMemoryRegions<'a> {
    list: &'a mut MemoryRegionList,
}

impl<'a> Iterator for DrainMemoryRegions<'a> {
    type Item = MemoryRegion;

    fn next(&mut self) -> Option<Self::Item> {
        if self.list.len == 0 {
            return None;
        }
        self.list.len -= 1;
        unsafe { Some(self.list.regions.add(self.list.len).read()) }
    }
}

struct ProcessTable {
    entries: *mut Process,
    len: usize,
    capacity: usize,
    next_pid: Pid,
    init_pid: Option<Pid>,
    idle_pid: Option<Pid>,
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
            idle_pid: None,
            initialized: false,
        }
    }

    fn spawn_kernel_process(
        &mut self,
        name: &'static str,
        parent: Option<Pid>,
        entry: ProcessEntry,
        is_idle: bool,
    ) -> Result<Pid, ProcessError> {
        let pid = self.allocate_pid()?;
        let process = Process::new_kernel(pid, name, parent, entry, is_idle)?;
        self.push(process)?;
        if is_idle {
            self.idle_pid = Some(pid);
        } else if self.init_pid.is_none() {
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

    fn remove_index(&mut self, index: usize) -> Process {
        assert!(index < self.len);
        unsafe {
            let removed = self.entries.add(index).read();
            if index != self.len - 1 {
                let moved = self.entries.add(self.len - 1).read();
                self.entries.add(index).write(moved);
            }
            self.len -= 1;
            if Some(removed.pid) == self.idle_pid {
                self.idle_pid = None;
            }
            if Some(removed.pid) == self.init_pid {
                self.init_pid = None;
            }
            removed
        }
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
        if self.len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.entries, self.len) }
        }
    }

    fn slice_mut(&mut self) -> &mut [Process] {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.entries, self.len) }
        }
    }

    fn find_index_by_pid(&self, pid: Pid) -> Option<usize> {
        self.slice().iter().position(|p| p.pid == pid)
    }

    fn has_child(&self, parent: Pid, target: Option<Pid>) -> bool {
        self.slice().iter().any(|process| {
            if process.parent != Some(parent) {
                return false;
            }
            if let Some(target_pid) = target {
                process.pid == target_pid
            } else {
                true
            }
        })
    }

    fn take_zombie_child(&mut self, parent: Pid, target: Option<Pid>) -> Option<(Pid, i32)> {
        for index in 0..self.len {
            unsafe {
                let entry_ptr = self.entries.add(index);
                if (*entry_ptr).parent != Some(parent) {
                    continue;
                }
                if (*entry_ptr).state != ProcessState::Zombie {
                    continue;
                }
                if let Some(target_pid) = target {
                    if (*entry_ptr).pid != target_pid {
                        continue;
                    }
                }

                let pid = (*entry_ptr).pid;
                let code = (*entry_ptr).exit_code.unwrap_or(0);
                let process = self.remove_index(index);
                drop(process);
                return Some((pid, code));
            }
        }
        None
    }

    fn next_ready_index(&self, start: Option<usize>) -> Option<usize> {
        if self.len == 0 {
            return None;
        }

        let slice = self.slice();
        let mut index = start.map(|i| (i + 1) % self.len).unwrap_or(0);
        let mut inspected = 0;
        let mut idle_candidate = None;
        while inspected < self.len {
            let process = &slice[index];
            match process.state {
                ProcessState::Ready => {
                    if process.is_idle {
                        if idle_candidate.is_none() {
                            idle_candidate = Some(index);
                        }
                    } else {
                        return Some(index);
                    }
                }
                ProcessState::Running => {
                    if process.is_idle && idle_candidate.is_none() {
                        idle_candidate = Some(index);
                    }
                }
                _ => {}
            }
            index = (index + 1) % self.len;
            inspected += 1;
        }
        idle_candidate
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
static NEED_RESCHED: AtomicBool = AtomicBool::new(false);

pub fn init() -> Result<(), ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if table.initialized {
        return Ok(());
    }
    table.initialized = true;
    let idle_pid = table.spawn_kernel_process("idle", None, idle_task, true)?;
    klog!("[process] table initialised idle_pid={}\n", idle_pid);
    Ok(())
}

pub fn spawn_kernel_process(name: &'static str, entry: ProcessEntry) -> Result<Pid, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if !table.initialized {
        return Err(ProcessError::NotInitialized);
    }
    let pid = table.spawn_kernel_process(name, current_pid(), entry, false)?;
    klog!("[process] spawned '{}' pid={}\n", name, pid);
    Ok(pid)
}

pub fn spawn_idle_process(name: &'static str, entry: ProcessEntry) -> Result<Pid, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if !table.initialized {
        return Err(ProcessError::NotInitialized);
    }
    if table.idle_pid.is_some() {
        return Err(ProcessError::IdleAlreadyExists);
    }
    let pid = table.spawn_kernel_process(name, None, entry, true)?;
    klog!("[process] spawned idle '{}' pid={}\n", name, pid);
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

fn reschedule() {
    while !schedule_internal() {
        core::hint::spin_loop();
    }
}

pub fn block_current(channel: WaitChannel) -> Result<(), ProcessError> {
    let pid = current_pid().ok_or(ProcessError::ProcessNotFound)?;
    {
        let mut table = PROCESS_TABLE.lock();
        let process = table.get_mut(pid).ok_or(ProcessError::ProcessNotFound)?;
        process.state = ProcessState::Blocked;
        process.wait_channel = Some(channel);
        process.preempt_return = None;
    }
    reschedule();
    Ok(())
}

pub fn wake_channel(event: WaitChannel) {
    let mut table = PROCESS_TABLE.lock();
    let slice = table.slice_mut();
    for process in slice {
        if process.state == ProcessState::Blocked {
            if let Some(channel) = process.wait_channel {
                if channel.matches_event(event) {
                    process.wait_channel = None;
                    process.state = ProcessState::Ready;
                    process.preempt_return = None;
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub fn request_preempt(frame: &mut InterruptFrame) {
    NEED_RESCHED.store(true, Ordering::Release);

    const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;

    let rip = frame.rip;

    if rip < KERNEL_BASE {
        return;
    }

    const KERNEL_TOP_BITS: u64 = 0xFFFF;
    if (rip >> 48) != KERNEL_TOP_BITS {
        let pid_for_log = current_pid().unwrap_or(0);
        /*
        klog!(
            "[request_preempt] non-canonical rip pid={} rip=0x{:016X} cs=0x{:X} rflags=0x{:016X}\n",
            pid_for_log,
            rip,
            frame.cs,
            frame.rflags
        );
        */

        unsafe {
            let raw = frame as *const InterruptFrame as *const u64;
            for i in 0..8 {
                let word = core::ptr::read(raw.add(i));
                //klog!("[request_preempt] stack[{i}] = 0x{:016X}\n", word);
            }
        }

        return;
    }

    let pid = match current_pid() {
        Some(pid) => pid,
        None => return,
    };

    let mut table = match PROCESS_TABLE.try_lock() {
        Some(guard) => guard,
        None => return,
    };

    let idx = match table.find_index_by_pid(pid) {
        Some(i) => i,
        None => return,
    };

    let slice = table.slice_mut();
    if let Some(process) = slice.get_mut(idx) {
        if process.state != ProcessState::Running || process.is_idle() {
            return;
        }
        if process.preempt_return.is_some() {
            return;
        }

        process.set_preempt_return(frame.rip);
        /*
        klog!(
            "[request_preempt] pid={} frame_rip=0x{:016X} return_rip=0x{:016X}\n",
            pid,
            frame.rip,
            process.context.rip
        );
        */
        frame.rip = preempt_trampoline as u64;
    }
}

#[no_mangle]
pub extern "C" fn preempt_do_switch() -> u64 {
    NEED_RESCHED.store(false, Ordering::Release);
    reschedule();

    let pid = current_pid().expect("preempted process missing current pid");
    let mut table = PROCESS_TABLE.lock();
    let process = table
        .get_mut(pid)
        .expect("preempted process missing from table");
    let target = process
        .take_preempt_return()
        .unwrap_or(process.context.rip);
        /*
    klog!(
        "[preempt] resume pid={} target_rip=0x{:016X} context_rip=0x{:016X}\n",
        pid,
        target,
        process.context.rip
    );
    */
    target
}

pub fn exit_current(exit_code: i32) -> ! {
    let pid = current_pid().expect("exit_current requires a running process");
    let parent = {
        let mut table = PROCESS_TABLE.lock();
        let process = table
            .get_mut(pid)
            .expect("current pid missing from table during exit");
        process.state = ProcessState::Zombie;
        process.wait_channel = None;
        process.exit_code = Some(exit_code);
        process.preempt_return = None;
        process.parent
    };

    if let Some(parent_pid) = parent {
        wake_channel(WaitChannel::Child(parent_pid));
    }

    reschedule();
    loop {
        core::hint::spin_loop();
    }
}

pub fn wait_for_child(target: Option<Pid>) -> Result<(Pid, i32), ProcessError> {
    let current = current_pid().ok_or(ProcessError::ProcessNotFound)?;

    loop {
        let should_block = {
            let mut table = PROCESS_TABLE.lock();
            if !table.has_child(current, target) {
                return if target.is_some() {
                    Err(ProcessError::ChildNotFound)
                } else {
                    Err(ProcessError::NoChildren)
                };
            }

            if let Some((pid, code)) = table.take_zombie_child(current, target) {
                return Ok((pid, code));
            }

            let process = table
                .get_mut(current)
                .ok_or(ProcessError::ProcessNotFound)?;
            let wait_channel = target
                .map(WaitChannel::Child)
                .unwrap_or(WaitChannel::ChildAny);
            process.state = ProcessState::Blocked;
            process.wait_channel = Some(wait_channel);
            process.preempt_return = None;
            true
        };

        if should_block {
            reschedule();
        }
    }
}

pub fn allocate_for_process(pid: Pid, layout: Layout, kind: MemoryRegionKind) -> Result<*mut u8, ProcessError> {
    allocate_for_process_with_permissions(pid, layout, kind, MemoryPermissions::read_write())
}

pub fn allocate_for_process_with_permissions(
    pid: Pid,
    layout: Layout,
    kind: MemoryRegionKind,
    permissions: MemoryPermissions,
) -> Result<*mut u8, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let process = table.get_mut(pid).ok_or(ProcessError::ProcessNotFound)?;
    process.allocate_region_with_permissions(layout, kind, permissions)
}

pub fn free_for_process(pid: Pid, ptr: *mut u8) -> Result<(), ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let process = table.get_mut(pid).ok_or(ProcessError::ProcessNotFound)?;
    process.release_region(ptr)
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
            process.cpu_slices = process.cpu_slices.saturating_add(1);
        }

        let next_pid = slice[next_index].pid;
        let next_ctx_ptr: *const Context = &slice[next_index].context;

        let current_ctx_ptr: *mut Context = match current_index {
            Some(idx) => &mut slice[idx].context as *mut Context,
            None => ptr::addr_of_mut!(BOOT_CONTEXT),
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

pub fn scheduler_stats() -> SchedulerStats {
    let table = PROCESS_TABLE.lock();
    let mut stats = SchedulerStats::empty();
    stats.need_resched = NEED_RESCHED.load(Ordering::Acquire);

    for process in table.slice() {
        stats.total += 1;
        stats.total_slices = stats.total_slices.saturating_add(process.cpu_slices);
        match process.state {
            ProcessState::Ready => stats.ready += 1,
            ProcessState::Running => stats.running += 1,
            ProcessState::Blocked => stats.blocked += 1,
            ProcessState::Zombie => stats.zombie += 1,
        }
    }

    stats
}

pub struct ProcessSnapshot {
    pid: Pid,
    parent: Option<Pid>,
    name: &'static str,
    state: ProcessState,
    cpu_slices: u64,
    is_idle: bool,
}

impl ProcessSnapshot {
    fn from(process: &Process) -> Self {
        Self {
            pid: process.pid,
            parent: process.parent,
            name: process.name,
            state: process.state,
            cpu_slices: process.cpu_slices,
            is_idle: process.is_idle,
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

    pub fn cpu_slices(&self) -> u64 {
        self.cpu_slices
    }

    pub fn is_idle(&self) -> bool {
        self.is_idle
    }
}

pub struct SchedulerStats {
    pub total: usize,
    pub ready: usize,
    pub running: usize,
    pub blocked: usize,
    pub zombie: usize,
    pub total_slices: u64,
    pub need_resched: bool,
}

impl SchedulerStats {
    const fn empty() -> Self {
        Self {
            total: 0,
            ready: 0,
            running: 0,
            blocked: 0,
            zombie: 0,
            total_slices: 0,
            need_resched: false,
        }
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
    {
        let table = PROCESS_TABLE.lock();
        let slice = table.slice();
        for process in slice {
            dump_process_inner(process);
        }
    }

    let stats = scheduler_stats();
    klog!(
        "[process] summary total={} ready={} running={} blocked={} zombie={} slices={} need_resched={}\n",
        stats.total,
        stats.ready,
        stats.running,
        stats.blocked,
        stats.zombie,
        stats.total_slices,
        stats.need_resched
    );
}

fn dump_process_inner(process: &Process) {
    klog!("[process] dump pid={} name='{}' state={} parent={:?}\n",
        process.pid,
        process.name,
        state_name(process.state),
        process.parent);
    klog!(
        "           wait={:?} exit_code={:?} idle={} preempt_ret={:?} slices={}\n",
        process.wait_channel,
        process.exit_code,
        process.is_idle,
        process.preempt_return,
        process.cpu_slices
    );

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

    for region in process.regions.iter() {
        let kind = match region.kind {
            MemoryRegionKind::Stack => "stack",
            MemoryRegionKind::Heap => "heap",
            MemoryRegionKind::Other => "other",
        };
        let read = if region.permissions.read() { 'r' } else { '-' };
        let write = if region.permissions.write() { 'w' } else { '-' };
        let exec = if region.permissions.execute() { 'x' } else { '-' };
        klog!(
            "           region {:>6} base=0x{:016X} size={:>6} align={} perms={}{}{}\n",
            kind,
            region.base as usize,
            region.layout.size(),
            region.layout.align(),
            read,
            write,
            exec
        );
    }
}
