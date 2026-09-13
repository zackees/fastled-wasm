//! FastLED manual-rebuild policy over kernel-owned terminal input.

use kernal_api::keys::{Key, KeyEvent, TerminalKeys};
use std::{io, time::Duration};

/// Lazy ownership lets the watch loop register graceful Ctrl+C handling before
/// changing terminal modes. Drop restores modes through the kernel owner.
#[derive(Default)]
pub(crate) struct RebuildKeys {
    input: Option<TerminalKeys>,
    attempted: bool,
}

impl RebuildKeys {
    /// Poll once. Noninteractive stdin is optional; other errors disable this
    /// input source permanently and are reported once to the caller.
    pub(crate) fn poll(&mut self) -> io::Result<bool> {
        if !self.attempted {
            self.attempted = true;
            self.input = match TerminalKeys::new() {
                Ok(input) => input,
                Err(error) if error.kind() == io::ErrorKind::Unsupported => None,
                Err(error) => return Err(error),
            };
        }
        let Some(input) = self.input.as_mut() else {
            return Ok(false);
        };
        match input.poll(Duration::ZERO) {
            Ok(event) => Ok(event.is_some_and(is_rebuild_key)),
            Err(error) => {
                self.input.take();
                Err(error)
            }
        }
    }
}

fn is_rebuild_key(event: KeyEvent) -> bool {
    matches!(event.key, Key::Character(' ') | Key::Enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernal_api::keys::KeyModifiers;

    #[test]
    fn only_space_and_enter_request_rebuilds() {
        for (key, expected) in [
            (Key::Character(' '), true),
            (Key::Enter, true),
            (Key::Character('x'), false),
            (Key::Escape, false),
            (Key::Other, false),
        ] {
            for modifiers in [
                KeyModifiers::default(),
                KeyModifiers {
                    control: true,
                    alt: true,
                },
            ] {
                assert_eq!(is_rebuild_key(KeyEvent { key, modifiers }), expected);
            }
        }
    }

    #[test]
    fn disabled_input_does_not_retry_capture() {
        let mut keys = RebuildKeys {
            input: None,
            attempted: true,
        };
        assert!(!keys.poll().unwrap());
        assert!(!keys.poll().unwrap());
    }
}
