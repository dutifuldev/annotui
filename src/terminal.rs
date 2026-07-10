use std::{
    fs::{File, OpenOptions},
    panic,
    sync::{
        atomic::{AtomicBool, Ordering},
        Once,
    },
};

use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type AppTerminal = Terminal<CrosstermBackend<File>>;

static PANIC_HOOK: Once = Once::new();
static RAW_ENABLED: AtomicBool = AtomicBool::new(false);
static SCREEN_ENABLED: AtomicBool = AtomicBool::new(false);
static MOUSE_ENABLED: AtomicBool = AtomicBool::new(false);
static PASTE_ENABLED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct TerminalGuard;

impl TerminalGuard {
    /// Enters raw alternate-screen mode on `/dev/tty`.
    ///
    /// # Errors
    ///
    /// Returns an error when the controlling terminal cannot be opened or configured.
    pub fn enter(mouse_enabled: bool) -> anyhow::Result<(Self, AppTerminal)> {
        install_panic_hook();
        let mut tty = open_tty()?;
        let guard = Self;
        enable_raw_mode()?;
        RAW_ENABLED.store(true, Ordering::SeqCst);
        execute!(tty, EnterAlternateScreen)?;
        SCREEN_ENABLED.store(true, Ordering::SeqCst);
        execute!(tty, EnableBracketedPaste)?;
        PASTE_ENABLED.store(true, Ordering::SeqCst);
        execute!(
            tty,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        )?;
        KEYBOARD_ENHANCED.store(true, Ordering::SeqCst);
        if mouse_enabled {
            execute!(tty, EnableMouseCapture)?;
            MOUSE_ENABLED.store(true, Ordering::SeqCst);
        }
        let terminal = Terminal::new(CrosstermBackend::new(tty))?;
        Ok((guard, terminal))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn open_tty() -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).open("/dev/tty")
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore_terminal();
            default_hook(info);
        }));
    });
}

fn restore_terminal() {
    let mut tty = open_tty().ok();
    if MOUSE_ENABLED.swap(false, Ordering::SeqCst) {
        if let Some(tty) = &mut tty {
            let _ = execute!(tty, DisableMouseCapture);
        }
    }
    if KEYBOARD_ENHANCED.swap(false, Ordering::SeqCst) {
        if let Some(tty) = &mut tty {
            let _ = execute!(tty, PopKeyboardEnhancementFlags);
        }
    }
    if PASTE_ENABLED.swap(false, Ordering::SeqCst) {
        if let Some(tty) = &mut tty {
            let _ = execute!(tty, DisableBracketedPaste);
        }
    }
    if SCREEN_ENABLED.swap(false, Ordering::SeqCst) {
        if let Some(tty) = &mut tty {
            let _ = execute!(tty, LeaveAlternateScreen);
        }
    }
    if RAW_ENABLED.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
    }
}
