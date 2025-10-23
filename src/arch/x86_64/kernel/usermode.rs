mod entry;

extern "C" {
    pub fn enter_user_mode() -> !;
}

pub fn trampoline() -> extern "C" fn() -> ! {
    unsafe { core::mem::transmute(enter_user_mode as *const ()) }
}
