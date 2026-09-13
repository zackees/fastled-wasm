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

/// CSS reference DPI; also what GTK reports once the GSettings schema is
/// visible and the text scaling factor is 1.0.
pub const FALLBACK_FONT_DPI: i32 = 96;

/// True when a `gtk-xft-dpi` value (DPI * 1024) is unusable. GDK reports the
/// DPI as -1 when unknown; WebKitGTK (2.52 and main) multiplies that by 1024,
/// checks only for exactly -1, and turns the result into a page zoom of
/// -1/96, which collapses layout and reports a negative devicePixelRatio.
pub fn font_dpi_is_unknown(gtk_xft_dpi: i32) -> bool {
    gtk_xft_dpi <= 0
}

/// The `gtk-xft-dpi` value (DPI * 1024) the viewer should run with.
///
/// WebKitGTK renders at GDK's integer scale factor and multiplies its page
/// zoom by font DPI / 96. Desktops with fractional scaling (KDE at 1.5 or
/// 1.75) tell GTK3 apps their intended scale through that font DPI, so the
/// effective scale should be `desktop_dpi / 96`, not the integer scale. That
/// means reporting `desktop_dpi / integer_scale`: at 168 dpi on a 2x display
/// the page zooms to 0.875 and lands at 1.75x. On X11 the integer scale is 1
/// and the value passes through unchanged. When the desktop gives no DPI at
/// all, report 96 so plain integer scaling applies and WebKit never sees
/// GDK's -1.
pub fn effective_font_dpi(desktop_gtk_xft_dpi: i32, integer_scale: i32) -> i32 {
    let scale = integer_scale.max(1);
    if font_dpi_is_unknown(desktop_gtk_xft_dpi) {
        return FALLBACK_FONT_DPI * 1024;
    }
    (desktop_gtk_xft_dpi / scale).max(1)
}

/// Applies [`effective_font_dpi`] to GTK before the webview exists. Setting
/// the `gtk-xft-dpi` setting also updates the GdkScreen resolution that
/// WebKitGTK's GTK3 build reads, and WebKit re-reads it on change. On Wayland
/// GDK only learns the desktop DPI from the GSettings
/// `org.gnome.desktop.interface` schema, which unwrapped binaries on some
/// distributions (NixOS with Plasma) cannot see; the GtkSettings property may
/// still carry the desktop's value from settings.ini, so it is read from
/// there and the screen resolution is corrected explicitly as well.
#[cfg(feature = "viewer")]
pub fn ensure_font_dpi() {
    use gtk::gdk::prelude::MonitorExt;
    use gtk::prelude::GtkSettingsExt;
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let desktop = settings.gtk_xft_dpi();
    let integer_scale = gtk::gdk::Display::default()
        .and_then(|display| display.primary_monitor().or_else(|| display.monitor(0)))
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1);
    let wanted = effective_font_dpi(desktop, integer_scale);
    let screen_dpi = gtk::gdk::Screen::default()
        .map(|screen| screen.resolution())
        .unwrap_or(-1.0);
    if wanted != desktop || screen_dpi.is_nan() || screen_dpi <= 0.0 {
        settings.set_gtk_xft_dpi(wanted);
        if let Some(screen) = gtk::gdk::Screen::default() {
            screen.set_resolution(f64::from(wanted) / 1024.0);
        }
        eprintln!(
            "fastled: font dpi {desktop} (desktop) / scale {integer_scale} -> {wanted}; screen resolution was {screen_dpi}"
        );
    }
}

/// Lets the page use the microphone. WebKitGTK ships with media streams
/// disabled and, unlike a browser, has no built-in permission prompt: every
/// `getUserMedia()` call is refused unless the embedding application enables
/// the setting and answers the `permission-request` signal. The viewer only
/// ever shows the user's own sketch, so user-media requests are allowed;
/// anything else (geolocation, notifications) is left to WebKit's default,
/// which is to deny.
#[cfg(feature = "viewer")]
pub fn allow_user_media(webview: &webkit2gtk::WebView) {
    use gtk::glib::Cast;
    use webkit2gtk::{PermissionRequestExt, SettingsExt, WebViewExt};
    if let Some(settings) = webview.settings() {
        settings.set_enable_media_stream(true);
        settings.set_enable_webaudio(true);
    }
    webview.connect_permission_request(|_, request| {
        if request
            .downcast_ref::<webkit2gtk::UserMediaPermissionRequest>()
            .is_some()
        {
            request.allow();
            true
        } else {
            false
        }
    });
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
    fn effective_font_dpi_follows_the_desktop_scale() {
        // Desktop at 1.75 (168 dpi) on a 2x integer display: zoom to 0.875
        assert_eq!(effective_font_dpi(172032, 2), 86016);
        // X11: integer scale 1, value passes through
        assert_eq!(effective_font_dpi(172032, 1), 172032);
        // 96 dpi desktop on a 2x display stays plain integer scaling
        assert_eq!(effective_font_dpi(98304, 2), 49152);
        // Unknown desktop DPI: 96 so WebKit never sees GDK's -1
        assert_eq!(effective_font_dpi(-1, 2), 98304);
        assert_eq!(effective_font_dpi(0, 1), 98304);
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
