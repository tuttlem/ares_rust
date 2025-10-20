pub mod loader;
pub mod elf;
mod fs;

use core::fmt;

pub type Uid = u32;
pub type Gid = u32;

pub const ROOT_UID: Uid = 0;
pub const ROOT_GID: Gid = 0;

pub mod space {
    pub const USER_ADDR_LIMIT: u64 = 0x0000_8000_0000;
    pub const DEFAULT_STACK_PAGES: usize = 8;
    pub const PAGE_SIZE: usize = 4096;

    pub const fn default_stack_size() -> usize {
        DEFAULT_STACK_PAGES * PAGE_SIZE
    }

    pub const fn stack_top() -> u64 {
        USER_ADDR_LIMIT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Credentials {
    real_uid: Uid,
    effective_uid: Uid,
    real_gid: Gid,
    effective_gid: Gid,
}

impl Credentials {
    pub const fn new(uid: Uid, gid: Gid) -> Self {
        Self {
            real_uid: uid,
            effective_uid: uid,
            real_gid: gid,
            effective_gid: gid,
        }
    }

    pub const fn root() -> Self {
        Self::new(ROOT_UID, ROOT_GID)
    }

    pub const fn real_uid(&self) -> Uid {
        self.real_uid
    }

    pub const fn effective_uid(&self) -> Uid {
        self.effective_uid
    }

    pub const fn real_gid(&self) -> Gid {
        self.real_gid
    }

    pub const fn effective_gid(&self) -> Gid {
        self.effective_gid
    }

    pub fn set_effective_uid(&mut self, uid: Uid) {
        self.effective_uid = uid;
    }

    pub fn set_effective_gid(&mut self, gid: Gid) {
        self.effective_gid = gid;
    }

    pub fn set_real_uid(&mut self, uid: Uid) {
        self.real_uid = uid;
    }

    pub fn set_real_gid(&mut self, gid: Gid) {
        self.real_gid = gid;
    }

    pub fn with_effective(mut self, uid: Uid, gid: Gid) -> Self {
        self.effective_uid = uid;
        self.effective_gid = gid;
        self
    }

    pub fn is_privileged(&self) -> bool {
        self.effective_uid == ROOT_UID
    }
}

impl fmt::Display for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "uid={}/{} gid={}/{}",
            self.real_uid,
            self.effective_uid,
            self.real_gid,
            self.effective_gid
        )
    }
}
