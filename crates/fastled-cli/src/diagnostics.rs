//! FastLED warning-color policy; encoding and native setup belong to the kernel.

use kernal_api::terminal_style::{self, Foreground, StyledText};
use std::sync::OnceLock;

pub(crate) fn yellow_warning(text: &str) -> StyledText<'_> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| {
        if no_color_requested(std::env::var("NO_COLOR").ok().as_deref()) {
            return false;
        }
        match terminal_style::prepare_stderr_ansi() {
            Ok(prepared) => prepared || std::env::var("TERM").is_ok_and(|term| term != "dumb"),
            Err(error) => {
                eprintln!("fastled: terminal color unavailable; using plain warnings: {error}");
                false
            }
        }
    });
    StyledText::new(text, Foreground::Yellow, enabled)
}

fn no_color_requested(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_policy_and_warning_content_are_preserved() {
        assert!(!no_color_requested(None));
        assert!(!no_color_requested(Some("")));
        assert!(no_color_requested(Some("1")));
        assert!(no_color_requested(Some("0")));
        let warning = "Warning: stale source\nUse source update";
        assert_eq!(
            StyledText::new(warning, Foreground::Yellow, false).to_string(),
            warning
        );
        assert_eq!(
            StyledText::new(warning, Foreground::Yellow, true).to_string(),
            format!("\x1b[38;5;11m{warning}\x1b[39m")
        );
    }
}
