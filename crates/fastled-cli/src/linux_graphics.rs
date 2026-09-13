//! Linux graphics workarounds for the WebKitGTK-backed Tauri viewer.
//!
//! WebKitGTK renders the page in a separate web process and hands finished
//! frames to the viewer over DMA-BUF. On NVIDIA that path has two known
//! failure modes, both observed on driver 595.71.05 / WebKitGTK 2.52.6:
//!
//! * Wayland: the window dies at once with "Error 71 (Protocol error)
//!   dispatching to Wayland display" because of the driver's explicit sync.
//!   `__NV_DISABLE_EXPLICIT_SYNC=1` keeps the fast DMA-BUF path working
//!   (Tauri's documented fix; UI-process CPU drops from ~50% to ~15%).
//! * X11: the viewer process cannot import the NVIDIA DMA-BUF and paints a
//!   solid grey window. Only `WEBKIT_DISABLE_DMABUF_RENDERER=1` paints, at
//!   the cost of a CPU copy of the window every frame.
//!
//! Separately, JavaScriptCore hides the `SharedArrayBuffer` global even when
//! the page is cross-origin isolated, which breaks Emscripten's pthread
//! bootstrap ("The object can not be cloned"). `JSC_useSharedArrayBuffer=1`
//! exposes it.
//!
//! Every override is applied only when the variable is unset, so a user who
//! sets one explicitly keeps their value. The decision is a pure function of
//! the host facts so it can be unit tested; `apply` gathers the facts and
//! writes the environment.

use std::collections::BTreeMap;

/// What the decision needs to know about the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    /// The proprietary NVIDIA kernel driver is loaded.
    pub nvidia: bool,
    /// GDK will use the X11 backend (explicit `GDK_BACKEND=x11`, or no
    /// Wayland display is available).
    pub x11_backend: bool,
    /// Variables the environment already carries; existing values win.
    pub already_set: Vec<String>,
}

/// Environment variables the viewer should set before creating its webview.
pub fn overrides(facts: &HostFacts) -> BTreeMap<&'static str, &'static str> {
    let mut result = BTreeMap::new();
    let mut set = |key: &'static str, value: &'static str| {
        if !facts.already_set.iter().any(|k| k == key) {
            result.insert(key, value);
        }
    };

    // WebKitGTK's JavaScriptCore keeps SharedArrayBuffer hidden; the
    // pthread WASM build needs it.
    set("JSC_useSharedArrayBuffer", "1");

    if facts.nvidia {
        set("__NV_DISABLE_EXPLICIT_SYNC", "1");
        if facts.x11_backend {
            set("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    result
}

/// Reads the host facts from `/proc` and the environment.
pub fn detect() -> HostFacts {
    let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/sys/module/nvidia").exists();
    let gdk_backend = std::env::var("GDK_BACKEND").unwrap_or_default();
    let x11_backend = if gdk_backend.is_empty() {
        std::env::var_os("WAYLAND_DISPLAY").is_none()
    } else {
        // GDK_BACKEND is a comma-separated preference list; the first wins.
        gdk_backend
            .split(',')
            .next()
            .is_some_and(|b| b.trim().eq_ignore_ascii_case("x11"))
    };
    let already_set = [
        "JSC_useSharedArrayBuffer",
        "__NV_DISABLE_EXPLICIT_SYNC",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
    ]
    .into_iter()
    .filter(|key| std::env::var_os(key).is_some())
    .map(str::to_string)
    .collect();
    HostFacts {
        nvidia,
        x11_backend,
        already_set,
    }
}

/// Applies the overrides to this process's environment and reports them on
/// stderr, so a user debugging a blank viewer can see what was changed.
pub fn apply() {
    let facts = detect();
    let overrides = overrides(&facts);
    for (key, value) in &overrides {
        std::env::set_var(key, value);
    }
    if !overrides.is_empty() {
        let list = overrides
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!(
            "fastled: viewer graphics workarounds applied (nvidia={}, x11={}): {list}",
            facts.nvidia, facts.x11_backend
        );
    }
}

/// Font DPI to report when GTK does not know it: CSS's reference DPI, the
/// value GTK itself uses once the GSettings schema is available.
pub const FALLBACK_FONT_DPI: i32 = 96;

/// True when a `gtk-xft-dpi` value (DPI * 1024) is unusable. GDK reports the
/// DPI as -1 when unknown; WebKitGTK (2.52 and main) multiplies that by 1024,
/// checks only for exactly -1, and turns the result into a page zoom of
/// -1/96, which collapses layout and reports a negative devicePixelRatio.
pub fn font_dpi_is_unknown(gtk_xft_dpi: i32) -> bool {
    gtk_xft_dpi <= 0
}

/// Gives GTK a usable font DPI when it reports none, so WebKitGTK computes a
/// sane page scale. Must run after GTK is initialised and before the webview
/// is created. WebKitGTK's GTK3 build reads `gdk_screen_get_resolution()`,
/// which on Wayland is only filled in from the GSettings
/// `org.gnome.desktop.interface` schema; unwrapped binaries on some
/// distributions (NixOS with Plasma) cannot see that schema, so the screen
/// keeps GDK's "unknown" value of -1 even when `gtk-xft-dpi` was set from
/// settings.ini. Both are corrected so GTK4 builds behave the same.
#[cfg(feature = "viewer")]
pub fn ensure_font_dpi() {
    use gtk::prelude::GtkSettingsExt;
    let dpi = f64::from(FALLBACK_FONT_DPI);
    if let Some(screen) = gtk::gdk::Screen::default() {
        let current = screen.resolution();
        if current.is_nan() || current <= 0.0 {
            screen.set_resolution(dpi);
            eprintln!(
                "fastled: GDK reported an unknown screen resolution ({current}); using {FALLBACK_FONT_DPI} dpi so WebKitGTK does not apply a negative page scale"
            );
        }
    }
    if let Some(settings) = gtk::Settings::default() {
        let current = settings.gtk_xft_dpi();
        if font_dpi_is_unknown(current) {
            settings.set_gtk_xft_dpi(FALLBACK_FONT_DPI * 1024);
            eprintln!(
                "fastled: GTK reported an unknown font DPI ({current}); using {FALLBACK_FONT_DPI}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(nvidia: bool, x11: bool, set: &[&str]) -> HostFacts {
        HostFacts {
            nvidia,
            x11_backend: x11,
            already_set: set.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn unknown_font_dpi_is_detected() {
        assert!(font_dpi_is_unknown(-1));
        assert!(font_dpi_is_unknown(-1024));
        assert!(font_dpi_is_unknown(0));
        assert!(!font_dpi_is_unknown(96 * 1024));
    }

    #[test]
    fn shared_array_buffer_is_always_exposed() {
        let o = overrides(&facts(false, false, &[]));
        assert_eq!(o.get("JSC_useSharedArrayBuffer"), Some(&"1"));
        assert_eq!(o.len(), 1, "no NVIDIA-specific overrides without NVIDIA");
    }

    #[test]
    fn nvidia_wayland_keeps_dmabuf_and_disables_explicit_sync() {
        let o = overrides(&facts(true, false, &[]));
        assert_eq!(o.get("__NV_DISABLE_EXPLICIT_SYNC"), Some(&"1"));
        assert!(!o.contains_key("WEBKIT_DISABLE_DMABUF_RENDERER"));
    }

    #[test]
    fn nvidia_x11_disables_dmabuf_renderer() {
        let o = overrides(&facts(true, true, &[]));
        assert_eq!(o.get("__NV_DISABLE_EXPLICIT_SYNC"), Some(&"1"));
        assert_eq!(o.get("WEBKIT_DISABLE_DMABUF_RENDERER"), Some(&"1"));
    }

    #[test]
    fn explicit_user_values_are_kept() {
        let o = overrides(&facts(
            true,
            true,
            &["WEBKIT_DISABLE_DMABUF_RENDERER", "JSC_useSharedArrayBuffer"],
        ));
        assert!(!o.contains_key("WEBKIT_DISABLE_DMABUF_RENDERER"));
        assert!(!o.contains_key("JSC_useSharedArrayBuffer"));
        assert_eq!(o.get("__NV_DISABLE_EXPLICIT_SYNC"), Some(&"1"));
    }
}
