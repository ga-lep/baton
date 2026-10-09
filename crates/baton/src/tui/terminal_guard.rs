//! Puts the host terminal into TUI mode and reliably puts it back.

use std::cell::Cell;
use std::io::{self, Write};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use crossterm::{execute, queue};

/// Whether keyboard enhancement flags were pushed and must be popped.
static ENHANCED: AtomicBool = AtomicBool::new(false);

/// Guards the one-time panic hook installation.
static HOOK: Once = Once::new();
/// How many times the hook was actually installed (always 0 or 1).
static HOOK_INSTALLS: AtomicUsize = AtomicUsize::new(0);

/// Chains a hook that restores the terminal before the previous hook runs.
/// Installed once per process, however often the guard is entered.
fn install_panic_hook() {
    HOOK.call_once(|| {
        HOOK_INSTALLS.fetch_add(1, Ordering::SeqCst);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if hook_should_restore() {
                restore();
            }
            previous(info);
        }));
    });
}

/// Whether a panic on the current thread should restore the terminal.
pub(crate) fn hook_should_restore() -> bool {
    !EXEMPT.with(Cell::get)
}

thread_local! {
    /// Set on helper threads whose panics must not touch the terminal.
    static EXEMPT: Cell<bool> = const { Cell::new(false) };
}

/// Exempts the calling thread from the terminal-restoring panic hook: a
/// panic in a detached helper thread must not tear down the running TUI.
pub(crate) fn exempt_current_thread() {
    EXEMPT.with(|e| e.set(true));
}

/// Undo everything [`TerminalGuard::enter`] did. Safe to call repeatedly.
fn restore() {
    let mut out = io::stdout();
    if ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableFocusChange,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// RAII guard: the terminal is restored on drop and on panic.
pub struct TerminalGuard {
    _private: (),
}

impl TerminalGuard {
    /// Enter raw mode and the alternate screen, and enable bracketed paste,
    /// focus change and mouse capture (plus keyboard enhancement if supported).
    ///
    /// # Errors
    /// Fails if the terminal cannot be configured; it is restored first.
    pub fn enter() -> io::Result<Self> {
        install_panic_hook();
        let guard = Self { _private: () };
        enable_raw_mode()?;
        // The query must happen in raw mode; it is false when unsupported.
        let enhanced = supports_keyboard_enhancement().unwrap_or(false);
        let mut out = io::stdout();
        queue!(
            out,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange,
            EnableMouseCapture
        )?;
        if enhanced {
            queue!(
                out,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                )
            )?;
            ENHANCED.store(true, Ordering::SeqCst);
        }
        out.flush()?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_hook_is_installed_only_once() {
        for _ in 0..3 {
            install_panic_hook();
        }
        assert_eq!(HOOK_INSTALLS.load(Ordering::SeqCst), 1);
    }
}
