//! Open a URL with LaunchServices, as `/usr/bin/open [-a <app>] <url>` would.

use std::ffi::c_void;
use std::process::ExitCode;
use std::ptr;

type CFTypeRef = *const c_void;
type CFURLRef = *const c_void;
type CFArrayRef = *const c_void;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_LS_LAUNCH_DEFAULTS: u32 = 0x0000_0001;

/// `LSLaunchURLSpec` from LaunchServices' LSOpen.h.
#[repr(C)]
struct LaunchUrlSpec {
    app_url: CFURLRef,
    item_urls: CFArrayRef,
    pass_thru_params: *const c_void,
    launch_flags: u32,
    async_ref_con: *mut c_void,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeArrayCallBacks: c_void;
    fn CFURLCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        base: CFURLRef,
    ) -> CFURLRef;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const CFTypeRef,
        count: isize,
        callbacks: *const c_void,
    ) -> CFArrayRef;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSOpenCFURLRef(url: CFURLRef, launched: *mut CFURLRef) -> i32;
    fn LSOpenFromURLSpec(spec: *const LaunchUrlSpec, launched: *mut CFURLRef) -> i32;
}

#[cfg(target_os = "macos")]
fn cf_url(text: &str) -> Option<CFURLRef> {
    // SAFETY: the byte slice outlives the call; null allocator and base select
    // the defaults.
    let url = unsafe {
        CFURLCreateWithBytes(
            ptr::null(),
            text.as_ptr(),
            text.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            ptr::null(),
        )
    };
    (!url.is_null()).then_some(url)
}

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (app, url) = match args.as_slice() {
        [url] => (None, url),
        [flag, app, url] if flag == "-a" => (Some(app), url),
        _ => {
            eprintln!("usage: open-url [-a <app bundle path>] <url>");
            return ExitCode::from(2);
        }
    };
    let Some(item) = cf_url(url) else {
        eprintln!("open-url: not a URL: {url}");
        return ExitCode::from(2);
    };
    let status = match app {
        // SAFETY: `item` is a valid CFURL.
        None => unsafe { LSOpenCFURLRef(item, ptr::null_mut()) },
        Some(app) => {
            let Some(app_url) = cf_url(&format!("file://{app}")) else {
                eprintln!("open-url: not an app path: {app}");
                return ExitCode::from(2);
            };
            let items = [item];
            // SAFETY: `items` holds one valid CFURL and outlives the call; the
            // spec's pointers are valid for the duration of the open.
            unsafe {
                let item_urls =
                    CFArrayCreate(ptr::null(), items.as_ptr(), 1, &kCFTypeArrayCallBacks);
                let spec = LaunchUrlSpec {
                    app_url,
                    item_urls,
                    pass_thru_params: ptr::null(),
                    launch_flags: K_LS_LAUNCH_DEFAULTS,
                    async_ref_con: ptr::null_mut(),
                };
                LSOpenFromURLSpec(&spec, ptr::null_mut())
            }
        }
    };
    println!(
        "open {url} via {} = {status}",
        app.map_or("default handler", |app| app)
    );
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
