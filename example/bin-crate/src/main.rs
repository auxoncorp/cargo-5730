#![feature(lang_items, core_intrinsics)]
#![feature(start)]
#![no_std]
extern crate libc;

use core::intrinsics;
use core::panic::PanicInfo;
use lib_crate;

#[start]
fn start(_argc: isize, _argv: *const *const u8) -> isize {
    let _a = lib_crate::add(1, 2);
    0
}

#[lang = "eh_personality"]
pub extern fn rust_eh_personality() {
}

#[lang = "eh_unwind_resume"]
pub extern fn rust_eh_unwind_resume() {
}

#[lang = "panic_impl"]
pub extern fn rust_begin_panic(_info: &PanicInfo) -> ! {
    unsafe { intrinsics::abort() }
}
