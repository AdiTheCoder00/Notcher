//! The notch: a frameless, transparent, always-on-top window pinned to the top
//! centre of the primary monitor.
//!
//! It must never take focus — dictation types into the app the user was using,
//! and a focused overlay would swallow the keystrokes. It is click-through in
//! transient states; the resting mark accepts hover, click, and wheel, and the
//! answer card needs the pointer for its copy button.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible, ShowWindow, SW_SHOW};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
};

pub const LABEL: &str = "notch";
pub const STATE_EVENT: &str = "notch://state";
pub const CONTROL_EVENT: &str = "control://state";

/// The idle window is only as wide as the resting mark, so its hover target
/// covers very little of the desktop. Other states grow just enough for their
/// content.
const IDLE_WIDTH: f64 = 200.0;
const IDLE_HEIGHT: f64 = 32.0;
/// The dictation bubble: a card, a gap, then the round mic badge.
const LISTENING_WIDTH: f64 = 640.0;
const LISTENING_HEIGHT: f64 = 136.0;
const THINKING_WIDTH: f64 = 292.0;
const THINKING_HEIGHT: f64 = 68.0;
const ANSWER_WIDTH: f64 = 560.0;
const ANSWER_HEIGHT: f64 = 300.0;
/// A short, single-line answer packs into one pill instead of the full card.
const ANSWER_PILL_WIDTH: f64 = 480.0;
const ANSWER_PILL_HEIGHT: f64 = 64.0;

/// Must match `isPillAnswer` in `src/components/notch/AnswerCard.tsx`.
pub fn is_pill_answer(text: Option<&str>) -> bool {
    text.map(|text| {
        let trimmed = text.trim();
        !trimmed.is_empty() && trimmed.chars().count() <= 90 && !trimmed.contains('\n')
    })
    .unwrap_or(false)
}
const ERROR_WIDTH: f64 = 448.0;
const ERROR_HEIGHT: f64 = 112.0;
/// Window width minus body width: a 12 px wing on each side of the panel, which
/// flares the top corners into the screen edge
/// (`html[data-window="notch"] .cc-overlay::before/::after`). Every Control
/// Centre panel is a body width plus this margin.
const SHOULDER_WINGS: f64 = 24.0;
/// The tabbed Control Centre grows downward from the notch.
pub const CONTROL_WIDTH: f64 = 700.0 + SHOULDER_WINGS;
pub const CONTROL_HEIGHT: f64 = 430.0;
/// Hovering the resting mark grows just the Now playing card: 127px of card
/// plus 4px above and 12px below, with no dead band left over.
const PEEK_WIDTH: f64 = 420.0;
const PEEK_HEIGHT: f64 = 143.0;
/// Gap between the top of the monitor and the pill.
const TOP_INSET: f64 = 0.0;

/// Set while the Control Centre owns the notch window. It keeps state updates
/// from shrinking the window underneath the open panel.
static CONTROL_OPEN: AtomicBool = AtomicBool::new(false);
/// Set while the hover media card owns the notch window.
static PEEK_OPEN: AtomicBool = AtomicBool::new(false);
static STATE_GENERATION: Mutex<u64> = Mutex::new(0);
static PREVIOUS_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);

/// Recover a taskbar left hidden by an older Murmur build without changing auto-hide.
pub fn restore_taskbar() {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    let flags = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) };
    if flags & ABS_AUTOHIDE as usize != 0 {
        return;
    }
    if let Ok(taskbar) = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) } {
        if !unsafe { IsWindowVisible(taskbar) }.as_bool() {
            let _ = unsafe { ShowWindow(taskbar, SW_SHOW) };
        }
        if let Err(err) = unsafe {
            SetWindowPos(
                taskbar,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        } {
            eprintln!("[notch] could not restore the Windows taskbar: {err}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotchState {
    /// Resting mark; click-through, always visible.
    Idle,
    Listening,
    Thinking,
    /// Expanded card: markdown answer with a copy button.
    Answer,
    Error,
}

impl NotchState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(NotchState::Idle),
            "listening" => Some(NotchState::Listening),
            "thinking" => Some(NotchState::Thinking),
            // `done` and `transcript` were the old names for the same card.
            "answer" | "done" | "transcript" => Some(NotchState::Answer),
            "error" => Some(NotchState::Error),
            _ => None,
        }
    }

    /// The resting mark accepts hover, click, or wheel to open Control Centre;
    /// the answer card accepts its copy button. The transient pills stay
    /// click-through.
    fn wants_pointer(self) -> bool {
        matches!(self, NotchState::Idle | NotchState::Answer)
    }

    fn size(self) -> (f64, f64) {
        match self {
            NotchState::Idle => (IDLE_WIDTH, IDLE_HEIGHT),
            NotchState::Listening => (LISTENING_WIDTH, LISTENING_HEIGHT),
            NotchState::Thinking => (THINKING_WIDTH, THINKING_HEIGHT),
            NotchState::Answer => (ANSWER_WIDTH, ANSWER_HEIGHT),
            NotchState::Error => (ERROR_WIDTH, ERROR_HEIGHT),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NotchPayload<'a> {
    pub state: NotchState,
    pub text: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlPayload {
    pub open: bool,
    /// "control" for the full panel, "peek" for the hover media card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
}

/// Builds the overlay window. Called once from setup.
pub fn create(app: &AppHandle) -> Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=notch".into()),
    )
    .title("Murmur")
    .inner_size(IDLE_WIDTH, IDLE_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()
    .context("create the notch window")?;

    // The small resting mark must receive hover events to open the panel.
    window
        .set_ignore_cursor_events(false)
        .context("enable hover on the resting mark")?;

    position(app)?;
    Ok(())
}

/// Centres the overlay on the primary monitor, flush with the top edge.
pub fn position(app: &AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    let Some(monitor) = app
        .primary_monitor()
        .map_err(|err| anyhow!("monitor lookup failed: {err}"))?
    else {
        eprintln!("[notch] no primary monitor; keeping the default position");
        return Ok(());
    };

    let scale = monitor.scale_factor();
    let origin = monitor.position();
    let size = monitor.size();
    // Read the window itself: its width changes for every state and for the
    // open Control Centre, so a compile-time constant would drift.
    let width_px = window.outer_size().context("read the notch size")?.width as i32;

    let x = origin.x + (size.width as i32 - width_px) / 2;
    let y = origin.y + (TOP_INSET * scale).round() as i32;

    window
        .set_position(PhysicalPosition::new(x, y))
        .context("position the notch")?;
    Ok(())
}

fn resize(app: &AppHandle, width: f64, height: f64) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    window
        .set_size(LogicalSize::new(width, height))
        .context("resize the notch")?;
    position(app)
}

fn remember_foreground() {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return;
    }
    if let Ok(mut previous) = PREVIOUS_FOREGROUND.lock() {
        *previous = Some(foreground.0 as isize);
    }
}

fn restore_foreground(notch: isize) {
    let previous = PREVIOUS_FOREGROUND
        .lock()
        .ok()
        .and_then(|mut previous| previous.take());
    if let Some(hwnd) = previous {
        if !should_restore_foreground(notch, unsafe { GetForegroundWindow() }.0 as isize) {
            return;
        }
        if !unsafe { SetForegroundWindow(HWND(hwnd as _)) }.as_bool() {
            eprintln!("[notch] Windows did not restore the previously focused window");
        }
    }
}

fn should_restore_foreground(notch: isize, foreground: isize) -> bool {
    notch != 0 && foreground == notch
}

/// Opens or closes the Control Centre inside the notch window. Opening grows
/// the window downward and makes the whole box interactive; closing returns to
/// the small resting mark.
pub fn set_control_open(app: &AppHandle, open: bool) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    if open {
        PEEK_OPEN.store(false, Ordering::SeqCst);
        if !CONTROL_OPEN.swap(true, Ordering::SeqCst) {
            remember_foreground();
        }
        window.show().context("show the notch")?;
        let _ = window.set_always_on_top(true);
        window
            .set_ignore_cursor_events(false)
            .context("make the Control Centre interactive")?;
        resize(app, CONTROL_WIDTH, CONTROL_HEIGHT)?;
    } else {
        CONTROL_OPEN.store(false, Ordering::SeqCst);
        PEEK_OPEN.store(false, Ordering::SeqCst);
        let reset = idle(app);
        restore_foreground(window.hwnd()?.0 as isize);
        reset?;
    }

    app.emit_to(
        LABEL,
        CONTROL_EVENT,
        ControlPayload {
            open,
            mode: open.then_some("control"),
        },
    )
    .map_err(|err| anyhow!("emit Control Centre state: {err}"))
}

/// Opens or closes the compact media card shown when the resting mark is
/// hovered. It never shrinks an open Control Centre.
pub fn set_peek_open(app: &AppHandle, open: bool) -> Result<()> {
    if CONTROL_OPEN.load(Ordering::SeqCst) {
        return Ok(());
    }

    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    if open {
        PEEK_OPEN.store(true, Ordering::SeqCst);
        window.show().context("show the notch")?;
        let _ = window.set_always_on_top(true);
        window
            .set_ignore_cursor_events(false)
            .context("make the media card interactive")?;
        resize(app, PEEK_WIDTH, PEEK_HEIGHT)?;
    } else {
        PEEK_OPEN.store(false, Ordering::SeqCst);
        idle(app)?;
    }

    app.emit_to(
        LABEL,
        CONTROL_EVENT,
        ControlPayload {
            open,
            mode: open.then_some("peek"),
        },
    )
    .map_err(|err| anyhow!("emit media card state: {err}"))
}

/// Focuses the compact Control Centre so the app search can accept typing.
pub fn set_launcher_open(app: &AppHandle) -> Result<()> {
    if !control_open() {
        set_control_open(app, true)?;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        window.set_focus().context("focus the launcher")?;
    }
    app.emit_to(LABEL, "control://focus", ())
        .context("focus the requested Control Centre control")
}

pub fn size_control_panel(app: &AppHandle, panel: &str) -> Result<()> {
    if !control_open() {
        return Ok(());
    }
    let (width, height): (f64, f64) = match panel {
        "control" => (CONTROL_WIDTH, 430.0),
        "today" => (900.0 + SHOULDER_WINGS, 460.0),
        "stats" => (900.0 + SHOULDER_WINGS, 380.0),
        "clipboard" => (900.0 + SHOULDER_WINGS, 460.0),
        "shelf" => (900.0 + SHOULDER_WINGS, 340.0),
        "apps" => (740.0 + SHOULDER_WINGS, 650.0),
        "wallpapers" => (1060.0 + SHOULDER_WINGS, 350.0),
        "power" => (CONTROL_WIDTH, 205.0),
        _ => return Err(anyhow!("Unknown Control Centre panel")),
    };
    if let Some(monitor) = app.primary_monitor()? {
        let scale = monitor.scale_factor();
        return resize(
            app,
            width.min(monitor.size().width as f64 / scale),
            height.min(monitor.size().height as f64 / scale),
        );
    }
    resize(app, width, height)
}

pub fn control_open() -> bool {
    CONTROL_OPEN.load(Ordering::SeqCst)
}

pub fn panels_open() -> bool {
    control_open() || PEEK_OPEN.load(Ordering::SeqCst)
}

/// Shows the overlay and pushes a state to it. Never calls `set_focus`.
pub fn show(app: &AppHandle, state: NotchState, text: Option<&str>) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    window.show().context("show the notch")?;
    if let Err(err) = window.set_always_on_top(true) {
        eprintln!("[notch] could not re-assert always-on-top: {err}");
    }

    update(app, state, text)
}

/// Pushes a state to the overlay and keeps click-through in sync with it.
pub fn update(app: &AppHandle, state: NotchState, text: Option<&str>) -> Result<()> {
    let mut generation = STATE_GENERATION
        .lock()
        .map_err(|_| anyhow!("notch state lock poisoned"))?;
    *generation = generation.wrapping_add(1);
    update_inner(app, state, text)
}

fn update_inner(app: &AppHandle, state: NotchState, text: Option<&str>) -> Result<()> {
    // The media card owns the window too; neither may shrink under the pointer.
    let control = panels_open();

    // The panel keeps its size while it is open; a background state update must
    // not shrink the window underneath it.
    if !control {
        let (width, height) = if state == NotchState::Answer && is_pill_answer(text) {
            (ANSWER_PILL_WIDTH, ANSWER_PILL_HEIGHT)
        } else {
            state.size()
        };
        if let Err(err) = resize(app, width, height) {
            eprintln!("[notch] could not resize the overlay: {err:#}");
        }
    }

    // Click-through is an OS window flag, so it has to follow the state here;
    // CSS `pointer-events` would not stop the window from eating desktop clicks.
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(err) = window.set_ignore_cursor_events(!(control || state.wants_pointer())) {
            eprintln!("[notch] could not change click-through: {err}");
        }
    }

    app.emit_to(LABEL, STATE_EVENT, NotchPayload { state, text })
        .map_err(|err| anyhow!("emit notch state: {err}"))
}

/// Returns to the resting mark. The window stays on screen and accepts hover.
pub fn idle(app: &AppHandle) -> Result<()> {
    update(app, NotchState::Idle, None)
}

/// Returns to idle after a delay, so a result stays readable first.
pub fn idle_later(app: AppHandle, delay: Duration) {
    let generation = match STATE_GENERATION.lock() {
        Ok(generation) => *generation,
        Err(_) => return,
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let Ok(mut current) = STATE_GENERATION.lock() else {
            return;
        };
        if !advance_if_current(&mut current, generation) {
            return;
        }
        if let Err(err) = update_inner(&app, NotchState::Idle, None) {
            eprintln!("[notch] could not return to idle: {err:#}");
        }
    });
}

fn advance_if_current(current: &mut u64, expected: u64) -> bool {
    if *current != expected {
        return false;
    }
    *current = current.wrapping_add(1);
    true
}

#[cfg(test)]
mod tests {
    use super::advance_if_current;

    #[test]
    fn closing_only_restores_focus_while_the_notch_owns_it() {
        assert!(super::should_restore_foreground(1, 1));
        assert!(!super::should_restore_foreground(1, 2));
        assert!(!super::should_restore_foreground(1, 0));
    }

    #[test]
    fn only_short_single_line_answers_use_the_pill() {
        assert!(super::is_pill_answer(Some("Here are boxing gloves.")));
        assert!(!super::is_pill_answer(Some("Two lines\nof answer")));
        assert!(!super::is_pill_answer(Some(&"x".repeat(91))));
        assert!(!super::is_pill_answer(Some("   ")));
        assert!(!super::is_pill_answer(None));
    }

    #[test]
    fn old_dismissal_cannot_replace_a_newer_state() {
        let mut generation = 2;
        assert!(!advance_if_current(&mut generation, 1));
        assert_eq!(generation, 2);
        assert!(advance_if_current(&mut generation, 2));
    }
}
