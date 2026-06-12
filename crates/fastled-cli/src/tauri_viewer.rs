use std::process::ExitCode;

pub struct ViewerOptions {
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Injected when `FASTLED_VIEWER_LOGS` is enabled: forwards `console.*`,
/// uncaught errors, unhandled rejections, and failed fetches to the FastLED
/// HTTP server's `POST /viewer-log` endpoint, which echoes them to stderr.
const LOG_FORWARD_SCRIPT: &str = r#"
(() => {
  const send = (line) => {
    try { fetch('/viewer-log', { method: 'POST', body: line }); } catch (_) {}
  };
  const fmt = (args) => Array.from(args).map((a) => {
    if (a instanceof Error) return a.stack || String(a);
    if (typeof a === 'object' && a !== null) {
      try { return JSON.stringify(a); } catch (_) { return String(a); }
    }
    return String(a);
  }).join(' ');
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    const original = console[level].bind(console);
    console[level] = (...args) => { original(...args); send(level + ': ' + fmt(args)); };
  }
  window.addEventListener('error', (e) => {
    send('window.onerror: ' + e.message + ' (' + e.filename + ':' + e.lineno + ')');
  });
  window.addEventListener('unhandledrejection', (e) => {
    send('unhandledrejection: ' + fmt([e.reason]));
  });
  const originalFetch = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const url = String(args[0] && args[0].url ? args[0].url : args[0]);
    if (url.endsWith('/viewer-log')) return originalFetch(...args);
    try {
      const resp = await originalFetch(...args);
      if (!resp.ok) send('fetch failed: ' + resp.status + ' ' + url);
      return resp;
    } catch (err) {
      send('fetch error: ' + url + ' ' + String(err));
      throw err;
    }
  };
})();
"#;

fn env_flag_enabled(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

fn viewer_logs_enabled() -> bool {
    let value = std::env::var("FASTLED_VIEWER_LOGS").ok();
    env_flag_enabled(value.as_deref())
}

pub fn run(options: ViewerOptions) -> ExitCode {
    let ViewerOptions {
        url,
        title,
        width,
        height,
    } = options;

    let result = tauri::Builder::default()
        .setup(move |app| {
            let url = tauri::WebviewUrl::External(url.parse()?);

            let mut builder = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title(&title)
                .inner_size(width as f64, height as f64);
            if viewer_logs_enabled() {
                builder = builder.initialization_script(LOG_FORWARD_SCRIPT);
            }
            let window = builder.build()?;

            // Counteract WebView2 DPI auto-scaling while keeping the UI readable.
            let scale = window.scale_factor().unwrap_or(1.0);
            if scale > 1.0 {
                let zoom = 0.92 / scale;
                let js = format!(
                    "document.addEventListener('DOMContentLoaded', function() {{ document.body.style.zoom = '{}'; }});",
                    zoom
                );
                window.eval(&js).ok();
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fastled: Tauri viewer failed: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_logs_flag_parsing() {
        assert!(!env_flag_enabled(None));
        assert!(!env_flag_enabled(Some("")));
        assert!(!env_flag_enabled(Some("0")));
        assert!(env_flag_enabled(Some("1")));
        assert!(env_flag_enabled(Some("true")));
    }

    #[test]
    fn log_forward_script_targets_server_endpoint() {
        assert!(LOG_FORWARD_SCRIPT.contains("/viewer-log"));
        assert!(LOG_FORWARD_SCRIPT.contains("unhandledrejection"));
        assert!(LOG_FORWARD_SCRIPT.contains("window.addEventListener('error'"));
    }
}
