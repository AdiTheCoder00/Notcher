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
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetForegroundWindow, IsWindow, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_HIDE, SW_SHOWNOACTIVATE,
};

pub const LABEL: &str = "notch";
pub const STATE_EVENT: &str = "notch://state";
pub const CONTROL_EVENT: &str = "control://state";

/// The idle window is only as wide as the resting mark, so its hover target
/// covers very little of the desktop. Other states grow just enough for their
/// content.
const IDLE_WIDTH: f64 = 184.0;
const IDLE_HEIGHT: f64 = 32.0;
const LISTENING_WIDTH: f64 = 396.0;
const LISTENING_HEIGHT: f64 = 76.0;
const THINKING_WIDTH: f64 = 292.0;
const THINKING_HEIGHT: f64 = 68.0;
const ANSWER_WIDTH: f64 = 560.0;
const ANSWER_HEIGHT: f64 = 300.0;
const ERROR_WIDTH: f64 = 448.0;
const ERROR_HEIGHT: f64 = 112.0;
/// The tabbed Control Centre grows downward from the notch.
pub const CONTROL_WIDTH: f64 = 880.0;
pub const CONTROL_HEIGHT: f64 = 620.0;
/// Gap between the top of the monitor and the pill.
const TOP_INSET: f64 = 0.0;

/// Set while the Control Centre owns the notch window. It keeps state updates
/// from shrinking the window underneath the open panel.
static CONTROL_OPEN: AtomicBool = AtomicBool::new(false);
static PREVIOUS_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);
static TASKBARS_TO_RESTORE: Mutex<Option<Vec<isize>>> = Mutex::new(None);

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

fn restore_foreground() {
    let previous = PREVIOUS_FOREGROUND
        .lock()
        .ok()
        .and_then(|mut previous| previous.take());
    if let Some(hwnd) = previous {
        if !unsafe { SetForegroundWindow(HWND(hwnd as _)) }.as_bool() {
            eprintln!("[notch] Windows did not restore the previously focused window");
        }
    }
}

fn hide_taskbars() -> Result<()> {
    let mut taskbars_to_restore = TASKBARS_TO_RESTORE
        .lock()
        .map_err(|_| anyhow!("taskbar state lock was poisoned"))?;
    if taskbars_to_restore.is_some() {
        return Ok(());
    }

    let mut hidden = Vec::new();
    for class_name in [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
        let mut after = HWND::default();
        while let Ok(taskbar) =
            unsafe { FindWindowExW(HWND::default(), after, class_name, PCWSTR::null()) }
        {
            after = taskbar;
            if !unsafe { IsWindowVisible(taskbar) }.as_bool() {
                continue;
            }

            hidden.push(taskbar.0 as isize);
            unsafe {
                let _ = ShowWindow(taskbar, SW_HIDE);
            }
            if unsafe { IsWindowVisible(taskbar) }.as_bool() {
                for hwnd in hidden {
                    unsafe {
                        let _ = ShowWindow(HWND(hwnd as _), SW_SHOWNOACTIVATE);
                    }
                }
                return Err(anyhow!("Windows did not hide the taskbar"));
            }
        }
    }

    *taskbars_to_restore = Some(hidden);
    Ok(())
}

pub fn restore_taskbars() -> Result<()> {
    let taskbars = TASKBARS_TO_RESTORE
        .lock()
        .map_err(|_| anyhow!("taskbar state lock was poisoned"))?
        .take();

    if let Some(taskbars) = taskbars {
        for hwnd in taskbars {
            let taskbar = HWND(hwnd as _);
            if unsafe { IsWindow(taskbar) }.as_bool() {
                unsafe {
                    let _ = ShowWindow(taskbar, SW_SHOWNOACTIVATE);
                }
            }
        }
    }
    Ok(())
}

/// Opens or closes the Control Centre inside the notch window. Opening grows
/// the window downward and makes the whole box interactive; closing returns to
/// the small resting mark.
pub fn set_control_open(app: &AppHandle, open: bool) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    if open {
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
        let reset = idle(app);
        let taskbars = restore_taskbars();
        restore_foreground();
        reset?;
        taskbars?;
    }

    app.emit_to(LABEL, CONTROL_EVENT, ControlPayload { open })
        .map_err(|err| anyhow!("emit Control Centre state: {err}"))
}

/// Expands the launcher over the primary display and temporarily hides taskbars.
pub fn set_launcher_open(app: &AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };

    let monitor = app
        .primary_monitor()
        .map_err(|err| anyhow!("monitor lookup failed: {err}"))?
        .ok_or_else(|| anyhow!("no primary monitor is available for the launcher"))?;
    let scale = monitor.scale_factor();
    let origin = monitor.position();
    let size = monitor.size();

    if !CONTROL_OPEN.swap(true, Ordering::SeqCst) {
        remember_foreground();
    }

    let opened = (|| -> Result<()> {
        hide_taskbars()?;
        window.show().context("show the launcher")?;
        window
            .set_always_on_top(true)
            .context("keep the launcher above other windows")?;
        window
            .set_ignore_cursor_events(false)
            .context("make the launcher interactive")?;
        window
            .set_size(LogicalSize::new(
                size.width as f64 / scale,
                size.height as f64 / scale,
            ))
            .context("expand the launcher to the primary display")?;
        window
            .set_position(PhysicalPosition::new(origin.x, origin.y))
            .context("position the launcher over the primary display")?;
        window.set_focus().context("focus the launcher")?;
        Ok(())
    })();

    if let Err(err) = opened {
        CONTROL_OPEN.store(false, Ordering::SeqCst);
        if let Err(reset_err) = idle(app) {
            eprintln!("[notch] could not restore the idle window: {reset_err:#}");
        }
        if let Err(taskbar_err) = restore_taskbars() {
            eprintln!("[notch] could not restore the taskbar: {taskbar_err:#}");
        }
        restore_foreground();
        return Err(err);
    }

    app.emit_to(LABEL, CONTROL_EVENT, ControlPayload { open: true })
        .map_err(|err| anyhow!("emit launcher state: {err}"))
}

pub fn control_open() -> bool {
    CONTROL_OPEN.load(Ordering::SeqCst)
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
    let control = CONTROL_OPEN.load(Ordering::SeqCst);

    // The panel keeps its size while it is open; a background state update must
    // not shrink the window underneath it.
    if !control {
        let (width, height) = state.size();
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
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        if let Err(err) = idle(&app) {
            eprintln!("[notch] could not return to idle: {err:#}");
        }
    });
}
