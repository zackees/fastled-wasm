//! Non-blocking keyboard input detection.
//!
//! Ports the core behaviour of `keyboard.py` (`SpaceBarWatcher`) to Rust
//! using the [`crossterm`] crate for cross-platform terminal raw-mode support.
//!
//! The public surface is intentionally minimal for Phase 2e:
//! * [`check_for_space`] — poll once and return immediately.
//! * [`start_listener`] — spawn a background thread and return a channel.
//!
//! The module is included as a library component; the CLI main loop will
//! integrate it in a later phase.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check once (non-blocking) whether a space bar key press is pending.
///
/// Returns `true` if a space (or Enter) is available in the terminal event
/// queue right now; `false` otherwise.
///
/// This function enables raw mode momentarily to read the event.  If the
/// terminal is not interactive (e.g. piped stdin in CI), it returns `false`
/// rather than panicking.
pub fn check_for_space() -> bool {
    // poll(Duration::ZERO) returns Ok(true) if an event is ready immediately.
    match terminal::enable_raw_mode() {
        Ok(()) => {}
        Err(_) => return false, // Not an interactive terminal — non-fatal.
    }
    let result = event::poll(Duration::ZERO)
        .map(|ready| {
            if !ready {
                return false;
            }
            matches!(
                event::read(),
                Ok(Event::Key(KeyEvent {
                    code: KeyCode::Char(' ') | KeyCode::Enter,
                    ..
                }))
            )
        })
        .unwrap_or(false);
    let _ = terminal::disable_raw_mode();
    result
}

/// Spawn a background thread that continuously reads keyboard events and
/// sends them through the returned channel.
#[allow(dead_code)]
///
/// The thread exits when the channel receiver is dropped or when a
/// `Ctrl+C` / `Esc` event is received.
///
/// # Platform notes
/// Raw mode is enabled for the duration of the listener's life.  The caller
/// is responsible for disabling it if needed after the receiver is dropped.
pub fn start_listener() -> mpsc::Receiver<KeyEvent> {
    let (tx, rx) = mpsc::channel::<KeyEvent>();

    thread::spawn(move || {
        if terminal::enable_raw_mode().is_err() {
            return; // Not interactive — just exit the thread.
        }

        loop {
            // Block up to 100 ms so we can yield the thread periodically.
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {
                    if let Ok(Event::Key(key)) = event::read() {
                        // Quit the listener on Ctrl+C or Esc.
                        let is_ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        let is_esc = key.code == KeyCode::Esc;

                        if tx.send(key).is_err() || is_ctrl_c || is_esc {
                            break;
                        }
                    }
                }
                Ok(false) => {
                    // Timeout — nothing to do; loop and poll again.
                    // The `tx.send(key).is_err()` path above handles receiver
                    // drop detection on the next real key event.
                }
                Err(_) => break,
            }
        }

        let _ = terminal::disable_raw_mode();
    });

    rx
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// In a non-interactive test environment (no TTY), `check_for_space`
    /// must return `false` without panicking.
    #[test]
    fn test_check_for_space_returns_false_when_no_key_pressed() {
        // CI / test runners pipe stdin, so raw mode will either fail or no
        // key will be available.  Either way we must get `false`.
        let result = check_for_space();
        assert!(
            !result,
            "expected false when no key pressed in non-interactive mode"
        );
    }

    /// `start_listener` must return a live receiver without panicking, even
    /// if the terminal is not interactive.
    #[test]
    fn test_start_listener_returns_receiver() {
        let rx = start_listener();
        // Drop the receiver immediately; the background thread must shut down
        // gracefully rather than panicking.
        drop(rx);
        // Give the thread a moment to exit cleanly.
        std::thread::sleep(Duration::from_millis(150));
    }
}
