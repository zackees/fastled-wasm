//! Open a URL with LaunchServices, as `/usr/bin/open <url>` would.

use std::ffi::c_void;
use std::process::ExitCode;
use std::ptr;

type CFURLRef = *const c_void;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        base: CFURLRef,
    ) -> CFURLRef;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSOpenCFURLRef(url: CFURLRef, launched: *mut CFURLRef) -> i32;
}

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("usage: open-url <url>");
        return ExitCode::from(2);
    };
    // SAFETY: the byte slice outlives the call, and a null allocator and base
    // select the defaults.
    let status = unsafe {
        let cf_url = CFURLCreateWithBytes(
            ptr::null(),
            url.as_ptr(),
            url.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            ptr::null(),
        );
        if cf_url.is_null() {
            eprintln!("open-url: not a URL: {url}");
            return ExitCode::from(2);
        }
        LSOpenCFURLRef(cf_url, ptr::null_mut())
    };
    println!("LSOpenCFURLRef({url}) = {status}");
    if status == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("open-url only runs on macOS");
    ExitCode::FAILURE
}
