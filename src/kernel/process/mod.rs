#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::drivers::{console, keyboard, CharDevice, DriverError};
use crate::klog;
use crate::mem::{heap, phys};
use crate::sync::spinlock::SpinLock;
use crate::user::{self, Credentials};
use crate::vfs::{VfsError, VfsFile};

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::kernel::interrupts::InterruptFrame;
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::kernel::{
    mmu,
    paging::{self, FLAG_NO_EXECUTE, FLAG_USER, FLAG_WRITABLE},
    usermode,
};

use core::alloc::Layout;
use core::array;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::{ptr, slice};

pub type Pid = u32;

pub const STDIN_FD: usize = 0;
pub const STDOUT_FD: usize = 1;
pub const STDERR_FD: usize = 2;
pub const SCRATCH_FD: usize = 3;
const MAX_FDS: usize = 16;
const KERNEL_STACK_SIZE: usize = 16 * 1024;

type ProcessEntry = extern "C" fn() -> !;

#[derive(Debug, Copy, Clone)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressSpaceKind {
    Kernel,
    User,
}

#[derive(Clone, Copy)]
pub struct AddressSpace {
    cr3: u64,
    kind: AddressSpaceKind,
}

impl AddressSpace {
    pub fn kernel() -> Self {
        #[cfg(target_arch = "x86_64")]
        let cr3 = unsafe { mmu::read_cr3() };

        #[cfg(not(target_arch = "x86_64"))]
        let cr3 = 0;

        Self {
            cr3,
            kind: AddressSpaceKind::Kernel,
        }
    }

    pub const fn with_cr3(cr3: u64, kind: AddressSpaceKind) -> Self {
        Self { cr3, kind }
    }

    pub const fn cr3(&self) -> u64 {
        self.cr3
    }

    pub const fn kind(&self) -> AddressSpaceKind {
        self.kind
    }

    pub const fn is_user(&self) -> bool {
        matches!(self.kind, AddressSpaceKind::User)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserStack {
    top: u64,
    size: usize,
}

impl UserStack {
    pub const fn new(top: u64, size: usize) -> Self {
        Self { top, size }
    }

    pub const fn top(&self) -> u64 {
        self.top
    }

    pub const fn size(&self) -> usize {
        self.size
    }

    pub const fn base(&self) -> u64 {
        self.top - self.size as u64
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
        if NEED_RESCHED.swap(false, Ordering::AcqRel) {
            if schedule_internal() {
                continue;
            }
        }

        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)); }
    }
}

pub enum FileDescriptor {
    Char(&'static dyn CharDevice),
    Vfs(VfsHandle),
}

pub struct VfsHandle {
    file: &'static dyn VfsFile,
    offset: u64,
}

impl VfsHandle {
    pub fn new(file: &'static dyn VfsFile) -> Self {
        Self { file, offset: 0 }
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let count = self.file.read_at(self.offset, buf)?;
        self.offset = self.offset.saturating_add(count as u64);
        Ok(count)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VfsError> {
        let count = self.file.write_at(self.offset, buf)?;
        self.offset = self.offset.saturating_add(count as u64);
        Ok(count)
    }

    fn flush(&self) -> Result<(), VfsError> {
        self.file.flush()
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, VfsError> {
        let size = self.file.size()?;
        let new_offset = match pos {
            SeekFrom::Start(pos) => pos,
            SeekFrom::Current(delta) => {
                let base = self.offset as i128 + delta as i128;
                if base < 0 {
                    return Err(VfsError::InvalidOffset);
                }
                base as u64
            }
            SeekFrom::End(delta) => {
                let base = size as i128 + delta as i128;
                if base < 0 {
                    return Err(VfsError::InvalidOffset);
                }
                base as u64
            }
        };

        if new_offset > size {
            return Err(VfsError::InvalidOffset);
        }

        self.offset = new_offset;
        Ok(new_offset)
    }
}

#[derive(Debug)]
pub enum FileIoError {
    Driver(DriverError),
    Vfs(VfsError),
}

impl From<DriverError> for FileIoError {
    fn from(err: DriverError) -> Self {
        FileIoError::Driver(err)
    }
}

impl From<VfsError> for FileIoError {
    fn from(err: VfsError) -> Self {
        FileIoError::Vfs(err)
    }
}

impl FileDescriptor {
    pub fn as_char(&self) -> Option<&'static dyn CharDevice> {
        match self {
            FileDescriptor::Char(device) => Some(*device),
            FileDescriptor::Vfs(_) => None,
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FileIoError> {
        match self {
            FileDescriptor::Char(device) => device.write(buf).map_err(FileIoError::from),
            FileDescriptor::Vfs(handle) => handle.write(buf).map_err(FileIoError::from),
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FileIoError> {
        match self {
            FileDescriptor::Char(device) => device.read(buf).map_err(FileIoError::from),
            FileDescriptor::Vfs(handle) => handle.read(buf).map_err(FileIoError::from),
        }
    }

    pub fn flush(&mut self) -> Result<(), FileIoError> {
        match self {
            FileDescriptor::Char(_) => Ok(()),
            FileDescriptor::Vfs(handle) => handle.flush().map_err(FileIoError::from),
        }
    }

    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64, FileIoError> {
        match self {
            FileDescriptor::Char(_) => Err(FileIoError::Driver(DriverError::Unsupported)),
            FileDescriptor::Vfs(handle) => handle.seek(pos).map_err(FileIoError::from),
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
    credentials: Credentials,
    address_space: AddressSpace,
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
    user_stack: Option<UserStack>,
    user_entry: Option<u64>,
}

impl Process {
    fn new_kernel(
        pid: Pid,
        name: &'static str,
        parent: Option<Pid>,
        entry: ProcessEntry,
        is_idle: bool,
        credentials: Credentials,
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

        let fds: [Option<FileDescriptor>; MAX_FDS] = array::from_fn(|_| None);

        let address_space = AddressSpace::kernel();

        let mut process = Self {
            pid,
            parent,
            name,
            credentials,
            address_space,
            state: ProcessState::Ready,
            wait_channel: None,
            exit_code: None,
            is_idle,
            preempt_return: None,
            cpu_slices: 0,
            fds,
            context,
            stack_ptr,
            stack_layout: Some(layout),
            regions: MemoryRegionList::new(),
            user_stack: None,
            user_entry: None,
        };

        let console_device = console::driver();
        process.set_fd(STDOUT_FD, FileDescriptor::Char(console_device))?;
        process.set_fd(STDERR_FD, FileDescriptor::Char(console_device))?;

        let keyboard_device = keyboard::driver();
        process.set_fd(STDIN_FD, FileDescriptor::Char(keyboard_device))?;

        if let Some(file) = crate::vfs::ata::AtaScratchFile::get() {
            process.set_fd(SCRATCH_FD, FileDescriptor::Vfs(VfsHandle::new(file)))?;
        }

        process.regions.register(MemoryRegion {
            base: stack_ptr,
            layout,
            kind: MemoryRegionKind::Stack,
            permissions: MemoryPermissions::read_write(),
        })?;

        Ok(process)
    }

    fn new_user(
        pid: Pid,
        name: &'static str,
        parent: Option<Pid>,
        path: &'static str,
        credentials: Credentials,
    ) -> Result<Self, ProcessError> {
        let (image, data) = user::loader::load_elf(path).map_err(|err| match err {
            user::loader::LoaderError::File(user::loader::FileError::NotFound) => ProcessError::PathNotFound,
            user::loader::LoaderError::File(_) => ProcessError::UserImageIo,
            user::loader::LoaderError::Elf(_) => ProcessError::InvalidElf,
        })?;

        let (address_space, user_stack) = create_default_user_address_space()?;

        map_user_segments(&address_space, &image, &data)?;

        let layout = Layout::from_size_align(KERNEL_STACK_SIZE, 16).map_err(|_| ProcessError::StackAllocationFailed)?;
        let stack_ptr = unsafe { heap::allocate(layout) };
        if stack_ptr.is_null() {
            return Err(ProcessError::StackAllocationFailed);
        }

        let stack_top = unsafe { stack_ptr.add(KERNEL_STACK_SIZE) } as u64;
        let mut aligned_top = stack_top & !0xFu64;

        let mut context = Context::new();
        context.rsp = aligned_top;
        context.rbp = aligned_top;
        context.rip = usermode::trampoline() as usize as u64;
        context.r15 = image.entry;
        context.r14 = user_stack.top();

        unsafe {
            aligned_top = aligned_top.saturating_sub(8);
            (aligned_top as *mut u64).write(process_exit as u64);
            context.rsp = aligned_top;
            context.rbp = aligned_top;
        }

        let fds: [Option<FileDescriptor>; MAX_FDS] = array::from_fn(|_| None);

        let mut process = Self {
            pid,
            parent,
            name,
            credentials,
            address_space,
            state: ProcessState::Ready,
            wait_channel: None,
            exit_code: None,
            is_idle: false,
            preempt_return: None,
            cpu_slices: 0,
            fds,
            context,
            stack_ptr,
            stack_layout: Some(layout),
            regions: MemoryRegionList::new(),
            user_stack: Some(user_stack),
            user_entry: Some(image.entry),
        };

        process.regions.register(MemoryRegion {
            base: stack_ptr,
            layout,
            kind: MemoryRegionKind::Stack,
            permissions: MemoryPermissions::read_write(),
        })?;

        let console_device = console::driver();
        process.set_fd(STDOUT_FD, FileDescriptor::Char(console_device))?;
        process.set_fd(STDERR_FD, FileDescriptor::Char(console_device))?;

        let keyboard_device = keyboard::driver();
        process.set_fd(STDIN_FD, FileDescriptor::Char(keyboard_device))?;

        if let Some(file) = crate::vfs::ata::AtaScratchFile::get() {
            process.set_fd(SCRATCH_FD, FileDescriptor::Vfs(VfsHandle::new(file)))?;
        }

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

    pub fn credentials(&self) -> Credentials {
        self.credentials
    }

    pub fn set_credentials(&mut self, credentials: Credentials) {
        self.credentials = credentials;
    }

    pub fn address_space(&self) -> AddressSpace {
        self.address_space
    }

    pub fn set_address_space(&mut self, space: AddressSpace) {
        self.address_space = space;
    }

    pub fn user_stack(&self) -> Option<UserStack> {
        self.user_stack
    }

    pub fn set_user_stack(&mut self, stack: Option<UserStack>) {
        self.user_stack = stack;
    }

    pub fn user_entry(&self) -> Option<u64> {
        self.user_entry
    }

    pub fn set_user_entry(&mut self, entry: Option<u64>) {
        self.user_entry = entry;
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

    fn allocate_fd_slot(&mut self, descriptor: FileDescriptor) -> Result<usize, ProcessError> {
        for (index, slot) in self.fds.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(descriptor);
                return Ok(index);
            }
        }
        Err(ProcessError::NoFreeFileDescriptors)
    }

    fn release_fd_slot(&mut self, index: usize) -> Result<FileDescriptor, ProcessError> {
        if index >= MAX_FDS {
            return Err(ProcessError::InvalidFileDescriptor);
        }
        self.fds[index]
            .take()
            .ok_or(ProcessError::InvalidFileDescriptor)
    }

    fn fd(&self, index: usize) -> Option<&FileDescriptor> {
        self.fds.get(index).and_then(|entry| entry.as_ref())
    }

    fn fd_mut(&mut self, index: usize) -> Option<&mut FileDescriptor> {
        self.fds.get_mut(index).and_then(|entry| entry.as_mut())
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
    PathNotFound,
    StackAllocationFailed,
    NotInitialized,
    MemoryRegionNotFound,
    IdleAlreadyExists,
    NoChildren,
    ChildNotFound,
    NoFreeFileDescriptors,
    AddressSpaceAllocationFailed,
    InvalidUserPointer,
    UserMemoryNotPresent,
    InvalidElf,
    UserImageIo,
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
        let credentials = if let Some(parent_pid) = parent {
            self.get(parent_pid)
                .map(|process| process.credentials)
                .unwrap_or_else(Credentials::root)
        } else {
            Credentials::root()
        };

        let process = Process::new_kernel(pid, name, parent, entry, is_idle, credentials)?;
        self.push(process)?;
        if is_idle {
            self.idle_pid = Some(pid);
        } else if self.init_pid.is_none() {
            self.init_pid = Some(pid);
        }
        Ok(pid)
    }

    fn spawn_user_process(
        &mut self,
        name: &'static str,
        parent: Option<Pid>,
        path: &'static str,
    ) -> Result<Pid, ProcessError> {
        let pid = self.allocate_pid()?;
        let credentials = if let Some(parent_pid) = parent {
            self.get(parent_pid)
                .map(|process| process.credentials)
                .unwrap_or_else(Credentials::root)
        } else {
            Credentials::root()
        };

        let process = Process::new_user(pid, name, parent, path, credentials)?;
        self.push(process)?;
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

pub fn spawn_user_process(name: &'static str, path: &'static str) -> Result<Pid, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    if !table.initialized {
        return Err(ProcessError::NotInitialized);
    }
    let parent = current_pid();
    let pid = table.spawn_user_process(name, parent, path)?;
    klog!("[process] spawned user '{}' pid={} path={}\n", name, pid, path);
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
    klog!("[process] starting scheduler\n");

    loop {
        if !schedule_internal() {
            core::hint::spin_loop();
        }
    }

    klog!("[process] exiting scheduler\n");
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
        let _pid_for_log = current_pid().unwrap_or(0);

        unsafe {
            let raw = frame as *const InterruptFrame as *const u64;
            for i in 0..8 {
                let _word = core::ptr::read(raw.add(i));
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

    klog!(
        "[preempt] resume pid={} target_rip=0x{:016X} context_rip=0x{:016X}\n",
        pid,
        target,
        process.context.rip
    );

    target
}

pub fn exit_current(exit_code: i32) -> ! {
    let pid = current_pid().expect("exit_current requires a running process");

    klog!("[process] exit request for pid {} as {}", pid, exit_code);

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
    let (current_ctx, next_ctx, current_space, next_space, next_pid) = {
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

        let current_space = current_index
            .and_then(|idx| slice.get(idx))
            .map(|process| process.address_space);
        let next_space = slice[next_index].address_space;

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

        (current_ctx_ptr, next_ctx_ptr, current_space, next_space, next_pid)
    };

    #[cfg(target_arch = "x86_64")]
    unsafe {
        let need_switch = match current_space {
            Some(space) => space.cr3() != next_space.cr3(),
            None => true,
        };
        if need_switch {
            mmu::write_cr3(next_space.cr3());
        }
    }

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
   credentials: Credentials,
   address_space: AddressSpace,
   user_stack: Option<UserStack>,
    user_entry: Option<u64>,
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
            credentials: process.credentials,
            address_space: process.address_space,
            user_stack: process.user_stack,
            user_entry: process.user_entry,
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

    pub fn credentials(&self) -> Credentials {
        self.credentials
    }

    pub fn address_space(&self) -> AddressSpace {
        self.address_space
    }

    pub fn user_stack(&self) -> Option<UserStack> {
        self.user_stack
    }

    pub fn user_entry(&self) -> Option<u64> {
        self.user_entry
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

pub fn current_credentials() -> Option<Credentials> {
    let pid = current_pid()?;
    let table = PROCESS_TABLE.lock();
    table.get(pid).map(|process| process.credentials())
}

pub fn current_address_space() -> Option<AddressSpace> {
    let pid = current_pid()?;
    let table = PROCESS_TABLE.lock();
    table.get(pid).map(|process| process.address_space())
}

fn ensure_user_range(ptr: u64, len: usize) -> Result<(), ProcessError> {
    if len == 0 {
        return Ok(());
    }

    let limit = mmu::KERNEL_VMA_BASE;

    if ptr >= limit {
        return Err(ProcessError::InvalidUserPointer);
    }

    let len_u64 = len as u64;
    let end = ptr
        .checked_add(len_u64)
        .ok_or(ProcessError::InvalidUserPointer)?;
    if end > limit {
        return Err(ProcessError::InvalidUserPointer);
    }
    Ok(())
}

fn copy_from_user_internal(
    address_space: &AddressSpace,
    dst: &mut [u8],
    user_ptr: u64,
) -> Result<(), ProcessError> {
    if dst.is_empty() {
        return Ok(());
    }

    ensure_user_range(user_ptr, dst.len())?;

    let mut copied = 0usize;
    while copied < dst.len() {
        let virt_addr = user_ptr + copied as u64;
        let phys = paging::translate(address_space.cr3(), virt_addr)
            .ok_or(ProcessError::UserMemoryNotPresent)?;

        let page_base = phys & !0xFFFu64;
        let page_offset = (phys & 0xFFFu64) as usize;
        let avail = paging::PAGE_SIZE - page_offset;
        let to_copy = core::cmp::min(avail, dst.len() - copied);

        let src_ptr = (mmu::phys_to_virt(page_base) as *const u8).wrapping_add(page_offset);
        unsafe {
            core::ptr::copy_nonoverlapping(src_ptr, dst.as_mut_ptr().add(copied), to_copy);
        }

        copied += to_copy;
    }

    Ok(())
}

fn copy_to_user_internal(
    address_space: &AddressSpace,
    user_ptr: u64,
    src: &[u8],
) -> Result<(), ProcessError> {
    if src.is_empty() {
        return Ok(());
    }

    ensure_user_range(user_ptr, src.len())?;

    let mut written = 0usize;
    while written < src.len() {
        let virt_addr = user_ptr + written as u64;
        let phys = paging::translate(address_space.cr3(), virt_addr)
            .ok_or(ProcessError::UserMemoryNotPresent)?;

        let page_base = phys & !0xFFFu64;
        let page_offset = (phys & 0xFFFu64) as usize;
        let avail = paging::PAGE_SIZE - page_offset;
        let to_copy = core::cmp::min(avail, src.len() - written);

        let dst_ptr = (mmu::phys_to_virt(page_base) as *mut u8).wrapping_add(page_offset);
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr().add(written), dst_ptr, to_copy);
        }

        written += to_copy;
    }

    Ok(())
}

pub fn copy_from_user(
    address_space: &AddressSpace,
    dst: &mut [u8],
    user_ptr: u64,
) -> Result<(), ProcessError> {
    match address_space.kind() {
        AddressSpaceKind::Kernel => {
            if dst.is_empty() {
                return Ok(());
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    user_ptr as *const u8,
                    dst.as_mut_ptr(),
                    dst.len(),
                );
            }
            Ok(())
        }
        AddressSpaceKind::User => copy_from_user_internal(address_space, dst, user_ptr),
    }
}

pub fn copy_to_user(
    address_space: &AddressSpace,
    user_ptr: u64,
    src: &[u8],
) -> Result<(), ProcessError> {
    match address_space.kind() {
        AddressSpaceKind::Kernel => {
            if src.is_empty() {
                return Ok(());
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src.as_ptr(),
                    user_ptr as *mut u8,
                    src.len(),
                );
            }
            Ok(())
        }
        AddressSpaceKind::User => copy_to_user_internal(address_space, user_ptr, src),
    }
}

pub fn read_user_buffer(
    address_space: &AddressSpace,
    user_ptr: u64,
    len: usize,
) -> Result<Vec<u8>, ProcessError> {
    let mut buffer = vec![0u8; len];
    copy_from_user(address_space, &mut buffer, user_ptr)?;
    Ok(buffer)
}

pub fn write_user_buffer(
    address_space: &AddressSpace,
    user_ptr: u64,
    data: &[u8],
) -> Result<(), ProcessError> {
    copy_to_user(address_space, user_ptr, data)
}

#[cfg(target_arch = "x86_64")]
pub fn create_user_address_space_with_stack(
    stack_pages: usize,
) -> Result<(AddressSpace, UserStack), ProcessError> {
    if stack_pages == 0 {
        return Err(ProcessError::AddressSpaceAllocationFailed);
    }

    let pml4_phys = paging::clone_kernel_pml4()
        .map_err(|_| ProcessError::AddressSpaceAllocationFailed)?;

    let address_space = AddressSpace::with_cr3(pml4_phys, AddressSpaceKind::User);

    let mut current_top = user::space::stack_top();
    let stack_size = stack_pages
        .checked_mul(paging::PAGE_SIZE)
        .ok_or(ProcessError::AddressSpaceAllocationFailed)?;

    for _ in 0..stack_pages {
        let frame = phys::allocate_frame().ok_or(ProcessError::AddressSpaceAllocationFailed)?;
        current_top = current_top.saturating_sub(paging::PAGE_SIZE as u64);
        paging::map_page(
            pml4_phys,
            current_top,
            frame.start(),
            FLAG_WRITABLE | FLAG_USER,
        )
        .map_err(|_| ProcessError::AddressSpaceAllocationFailed)?;
    }

    let user_stack = UserStack::new(user::space::stack_top(), stack_size);
    Ok((address_space, user_stack))
}

#[cfg(target_arch = "x86_64")]
pub fn create_default_user_address_space() -> Result<(AddressSpace, UserStack), ProcessError> {
    create_user_address_space_with_stack(user::space::DEFAULT_STACK_PAGES)
}

fn map_user_segments(
    address_space: &AddressSpace,
    image: &user::elf::ElfImage,
    data: &[u8],
) -> Result<(), ProcessError> {
    for segment in &image.segments {
        let start = align_down(segment.vaddr, paging::PAGE_SIZE as u64);
        let end = align_up(segment.vaddr + segment.memsz, paging::PAGE_SIZE as u64);

        let mut page = start;
        while page < end {
            let frame = phys::allocate_frame().ok_or(ProcessError::AddressSpaceAllocationFailed)?;
            let frame_ptr = mmu::phys_to_virt(frame.start()) as *mut u8;
            unsafe {
                ptr::write_bytes(frame_ptr, 0, paging::PAGE_SIZE);
            }

            let mut flags = FLAG_USER;
            if user::elf::segment_flags_writable(segment.flags) {
                flags |= FLAG_WRITABLE;
            }
            if !user::elf::segment_flags_executable(segment.flags) {
                flags |= FLAG_NO_EXECUTE;
            }

            paging::map_page(address_space.cr3(), page, frame.start(), flags)
                .map_err(|_| ProcessError::AddressSpaceAllocationFailed)?;

            let seg_file_end = segment.vaddr + segment.filesz;
            let copy_start = core::cmp::max(segment.vaddr, page);
            let copy_end = core::cmp::min(seg_file_end, page + paging::PAGE_SIZE as u64);

            if copy_end > copy_start {
                let dst_offset = (copy_start - page) as usize;
                let src_offset = (copy_start - segment.vaddr) as usize;
                let len = (copy_end - copy_start) as usize;

                let src_index = segment.offset as usize + src_offset;
                if src_index + len > data.len() {
                    return Err(ProcessError::InvalidElf);
                }

                unsafe {
                    ptr::copy_nonoverlapping(
                        data.as_ptr().add(src_index),
                        frame_ptr.add(dst_offset),
                        len,
                    );
                }
            }

            page = page.saturating_add(paging::PAGE_SIZE as u64);
        }
    }

    Ok(())
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    if value & (align - 1) == 0 {
        value
    } else {
        (value | (align - 1)).saturating_add(1)
    }
}

pub fn open_path(pid: Pid, path: &str) -> Result<usize, ProcessError> {
    let descriptor = if path.starts_with("/fat/") {
        let sub = &path[5..];
        let file = crate::fs::fat::open_file(sub).map_err(|err| match err {
            crate::fs::fat::FatError::NotMounted => ProcessError::PathNotFound,
            crate::fs::fat::FatError::InvalidPath => ProcessError::PathNotFound,
            crate::fs::fat::FatError::NotFound => ProcessError::PathNotFound,
            crate::fs::fat::FatError::Io => ProcessError::AllocationFailed,
        })?;
        FileDescriptor::Vfs(VfsHandle::new(file))
    } else {
        match path {
            "/scratch" => {
                let file = crate::vfs::ata::AtaScratchFile::get().ok_or(ProcessError::PathNotFound)?;
                FileDescriptor::Vfs(VfsHandle::new(file))
            }
            "/dev/console" => FileDescriptor::Char(console::driver()),
            "/dev/null" => {
                let dev = crate::drivers::char_device_by_name("null").ok_or(ProcessError::PathNotFound)?;
                FileDescriptor::Char(dev)
            }
            "/dev/zero" => {
                let dev = crate::drivers::char_device_by_name("zero").ok_or(ProcessError::PathNotFound)?;
                FileDescriptor::Char(dev)
            }
            _ => return Err(ProcessError::PathNotFound),
        }
    };

    let mut table = PROCESS_TABLE.lock();
    let process = table
        .get_mut(pid)
        .ok_or(ProcessError::ProcessNotFound)?;
    process.allocate_fd_slot(descriptor)
}

pub fn close_fd(pid: Pid, fd: usize) -> Result<(), ProcessError> {
    let descriptor = {
        let mut table = PROCESS_TABLE.lock();
        let process = table
            .get_mut(pid)
            .ok_or(ProcessError::ProcessNotFound)?;
        process.release_fd_slot(fd)?
    };

    let mut descriptor = descriptor;
    if let Err(err) = descriptor.flush() {
        klog!("[process] flush on close failed: {:?}\n", err);
    }
    Ok(())
}

pub fn with_fd_mut<F, R>(pid: Pid, fd: usize, f: F) -> Result<R, ProcessError>
where
    F: FnOnce(&mut FileDescriptor) -> R,
{
    let mut descriptor = {
        let mut table = PROCESS_TABLE.lock();
        let process = table
            .get_mut(pid)
            .ok_or(ProcessError::ProcessNotFound)?;
        let slot = process
            .fds
            .get_mut(fd)
            .ok_or(ProcessError::InvalidFileDescriptor)?;
        slot.take().ok_or(ProcessError::InvalidFileDescriptor)?
    };

    let result = f(&mut descriptor);

    {
        let mut table = PROCESS_TABLE.lock();
        let process = table
            .get_mut(pid)
            .ok_or(ProcessError::ProcessNotFound)?;
        let slot = process
            .fds
            .get_mut(fd)
            .ok_or(ProcessError::InvalidFileDescriptor)?;
        debug_assert!(slot.is_none(), "fd slot occupied during restore");
        *slot = Some(descriptor);
    }

    Ok(result)
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
        "           credentials {} addr_space={:?} cr3=0x{:016X}\n",
        process.credentials,
        process.address_space.kind(),
        process.address_space.cr3()
    );
    if let Some(stack) = process.user_stack {
        klog!(
            "           user_stack base=0x{:016X} top=0x{:016X} size={}\n",
            stack.base(),
            stack.top(),
            stack.size()
        );
    }
    if let Some(entry) = process.user_entry {
        klog!("           user_entry=0x{:016X}\n", entry);
    }
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
                FileDescriptor::Vfs(handle) => {
                    klog!(
                        "           fd {:>2}: VfsFile '{}' offset={}\n",
                        fd,
                        handle.file.name(),
                        handle.offset
                    );
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
