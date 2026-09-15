//! Embeds the Windows executable's icon, version information and application
//! manifest. Only the binary's own build script can link these resources, so
//! the mechanics live in kernal-api and this file only declares the product.

use kernal_api::windows_resources::{embed_windows_app_resources, WindowsAppResources};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let resources =
        match WindowsAppResources::new("FastLED Viewer", "fastled", env!("CARGO_PKG_VERSION")) {
            Ok(resources) => resources.with_icon("icons/icon.ico"),
            Err(error) => panic!("fastled: invalid Windows resource metadata: {error}"),
        };
    if let Err(error) = embed_windows_app_resources(&resources) {
        panic!("fastled: {error}");
    }
}
