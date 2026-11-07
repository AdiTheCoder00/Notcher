//! Hold-to-talk hotkeys via a low-level keyboard hook.
//!
//! `RegisterHotKey` — and therefore `tauri-plugin-global-shortcut` — needs a
//! real virtual key: modifier-only chords like Ctrl+Win cannot be registered,
//! and a hold-to-talk gesture needs both edges anyway. So a dedicated thread
//! installs `WH_KEYBOARD_LL`, keeps its own message loop, and reports
//! press/release edges for the configured chords.
//!
//! Windows opens the Start menu on the *release* of a lone Win key. When our
//! Ctrl+Win binding ends, the hook swallows that key-up so the menu never
//! appears. The key-up is only swallowed when no other key was pressed during
//! the Win hold, so real Windows shortcuts (Win+E, Ctrl+Win+arrows) still work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::thread;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

/// Emitted to the UI on every modifier or gesture change.
pub const STATE_EVENT: &str = "hotkeys://state";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    /// Ctrl+Win — dictate into the focused application.
    Dictation,
    /// Ctrl+Shift — ask about the region under the cursor.
    CircleToAsk,
    /// Ctrl+Alt — schedule a task in natural language.
    ScheduleTask,
}

impl Gesture {
    /// The three gestures in Loadout order.
    pub const ALL: [Gesture; 3] = [
        Gesture::Dictation,
        Gesture::CircleToAsk,
        Gesture::ScheduleTask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Gesture::Dictation => "Dictation",
            Gesture::CircleToAsk => "Circle to ask",
            Gesture::ScheduleTask => "Schedule by voice",
        }
    }

    pub fn listening_label(self) -> &'static str {
        match self {
            Gesture::Dictation => "Listening…",
            Gesture::CircleToAsk => "Listening — ask about the screen…",
            Gesture::ScheduleTask => "Listening — what should I schedule?",
        }
    }
}

/// Current physical modifier state, as the hook sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

/// A chord → gesture mapping. Bindings match exactly, so releasing one
/// modifier rolls cleanly into another chord instead of double-firing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub gesture: Gesture,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

impl Binding {
    fn matches(&self, modifiers: Modifiers) -> bool {
        self.ctrl == modifiers.ctrl
            && self.shift == modifiers.shift
            && self.alt == modifiers.alt
            && self.win == modifiers.win
    }
}

/// The bindings the hook resolves against. Rebinding UI comes later; the shape
/// is already config-driven so it can come from settings without touching the
/// hook.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    pub bindings: Vec<Binding>,
}

impl HotkeyConfig {
    pub fn new(bindings: Vec<Binding>) -> Self {
        Self { bindings }
    }

    fn resolve(&self, modifiers: Modifiers) -> Option<Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.matches(modifiers))
            .copied()
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            bindings: vec![
                Binding {
                    gesture: Gesture::Dictation,
                    ctrl: true,
                    shift: false,
                    alt: false,
                    win: true,
                },
                Binding {
                    gesture: Gesture::CircleToAsk,
                    ctrl: true,
                    shift: true,
                    alt: false,
                    win: false,
                },
                Binding {
                    gesture: Gesture::ScheduleTask,
                    ctrl: true,
                    shift: false,
                    alt: true,
                    win: false,
                },
            ],
        }
    }
}

/// Snapshot for the debug panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HotkeySnapshot {
    pub gesture: Option<Gesture>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

impl HotkeySnapshot {
    const IDLE: Self = Self {
        gesture: None,
        ctrl: false,
        shift: false,
        alt: false,
        win: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed(Gesture),
    Released(Gesture),
    State(HotkeySnapshot),
}

static SENDER: OnceLock<UnboundedSender<HotkeyEvent>> = OnceLock::new();
/// The hook reads this on every key edge, so a Loadout edit takes effect
/// without restarting the listener thread.
static CONFIG: OnceLock<RwLock<HotkeyConfig>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Bindings currently in force, for the settings window.
pub fn current_bindings() -> Vec<Binding> {
    CONFIG
        .get()
        .and_then(|config| config.read().ok())
        .map(|config| config.bindings.clone())
        .unwrap_or_else(|| HotkeyConfig::default().bindings)
}

/// Swaps the bindings the installed hook resolves against.
pub fn set_bindings(bindings: Vec<Binding>) -> Result<()> {
    let config = CONFIG
        .get()
        .ok_or_else(|| anyhow!("the hotkey listener is not running"))?;
    let mut guard = config
        .write()
        .map_err(|_| anyhow!("the hotkey config is poisoned"))?;
    guard.bindings = bindings;
    Ok(())
}

/// Installs the hook on its own thread. Fails only if the thread cannot start;
/// a hook that Windows refuses to install is logged and leaves the app running.
pub fn spawn(config: HotkeyConfig, sender: UnboundedSender<HotkeyEvent>) -> Result<()> {
    if CONFIG.set(RwLock::new(config)).is_err() || SENDER.set(sender).is_err() {
        return Err(anyhow!("the hotkey listener is already running"));
    }
    RUNNING.store(true, Ordering::SeqCst);
    install()
}

pub fn stop() {
    RUNNING.store(false, Ordering::SeqCst);
    #[cfg(windows)]
    win::stop();
}

#[cfg(windows)]
fn install() -> Result<()> {
    thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(win::run)
        .map_err(|err| anyhow!("spawn the keyboard hook thread: {err}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn install() -> Result<()> {
    RUNNING.store(false, Ordering::SeqCst);
    Err(anyhow!("global hotkeys are only implemented on Windows"))
}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::{Binding, HotkeyEvent, HotkeySnapshot, Modifiers, CONFIG, RUNNING, SENDER};

    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_LSHIFT: u32 = 0xA0;
    const VK_RSHIFT: u32 = 0xA1;
    const VK_LCONTROL: u32 = 0xA2;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_LMENU: u32 = 0xA4;
    const VK_RMENU: u32 = 0xA5;
    const VK_LWIN: u32 = 0x5B;
    const VK_RWIN: u32 = 0x5C;

    struct HookState {
        ctrl: bool,
        shift: bool,
        alt: bool,
        win: bool,
        active: Option<super::Gesture>,
        /// Set when a Win binding fires; consumed by the matching Win key-up.
        swallow_win_release: bool,
        /// Another key was pressed during this Win hold, so Windows itself
        /// would not open the Start menu and we must not swallow the key-up.
        other_key_seen: bool,
        last_sent: HotkeySnapshot,
    }

    impl HookState {
        const fn new() -> Self {
            Self {
                ctrl: false,
                shift: false,
                alt: false,
                win: false,
                active: None,
                swallow_win_release: false,
                other_key_seen: false,
                last_sent: HotkeySnapshot::IDLE,
            }
        }

        fn modifiers(&self) -> Modifiers {
            Modifiers {
                ctrl: self.ctrl,
                shift: self.shift,
                alt: self.alt,
                win: self.win,
            }
        }
    }

    // Only touched from the hook thread, and only for a few instructions.
    static STATE: Mutex<HookState> = Mutex::new(HookState::new());
    static THREAD_ID: AtomicU32 = AtomicU32::new(0);

    pub fn run() {
        // SAFETY: WH_KEYBOARD_LL with a callback defined in this module, a NULL
        // module handle (required when the callback lives in this process and
        // dwThreadId is 0) and its own message loop on this thread.
        unsafe {
            let hook =
                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None::<&HINSTANCE>, 0)
                {
                    Ok(hook) => hook,
                    Err(err) => {
                        eprintln!("[hotkeys] could not install the low-level keyboard hook: {err}");
                        RUNNING.store(false, Ordering::SeqCst);
                        return;
                    }
                };

            THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);

            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, None::<&HWND>, 0, 0);
                if result.0 <= 0 {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            if let Err(err) = UnhookWindowsHookEx(hook) {
                eprintln!("[hotkeys] could not remove the keyboard hook: {err}");
            }
            THREAD_ID.store(0, Ordering::SeqCst);
        }
    }

    pub fn stop() {
        let thread_id = THREAD_ID.load(Ordering::SeqCst);
        if thread_id == 0 {
            return;
        }
        // SAFETY: posts a quit message to our own hook thread's queue.
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            // HC_ACTION is 0; negative means "pass it on without inspection".
            return unsafe { CallNextHookEx(None::<&HHOOK>, code, wparam, lparam) };
        }

        // SAFETY: for HC_ACTION the system hands us a KBDLLHOOKSTRUCT pointer.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

        if info.flags.0 & LLKHF_INJECTED.0 != 0 {
            // Our own enigo typing must never re-enter the gesture machine.
            return unsafe { CallNextHookEx(None::<&HHOOK>, code, wparam, lparam) };
        }

        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;

        if !down && !up {
            return unsafe { CallNextHookEx(None::<&HHOOK>, code, wparam, lparam) };
        }

        let swallow = match STATE.lock() {
            Ok(mut state) => apply_key(&mut state, info.vkCode, down),
            Err(_) => false,
        };

        if swallow {
            return LRESULT(1);
        }

        unsafe { CallNextHookEx(None::<&HHOOK>, code, wparam, lparam) }
    }

    /// Updates modifier state, resolves the chord and reports edges.
    /// Returns true when this key event must not reach the rest of Windows.
    fn apply_key(state: &mut HookState, vk: u32, down: bool) -> bool {
        let is_win_key = matches!(vk, VK_LWIN | VK_RWIN);
        let is_modifier = is_win_key
            || matches!(
                vk,
                VK_CONTROL
                    | VK_LCONTROL
                    | VK_RCONTROL
                    | VK_SHIFT
                    | VK_LSHIFT
                    | VK_RSHIFT
                    | VK_MENU
                    | VK_LMENU
                    | VK_RMENU
            );

        match vk {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => state.ctrl = down,
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => state.shift = down,
            VK_MENU | VK_LMENU | VK_RMENU => state.alt = down,
            VK_LWIN | VK_RWIN => state.win = down,
            _ => {}
        }

        if down && is_win_key {
            state.other_key_seen = false;
        }
        if down && !is_modifier && state.win {
            state.other_key_seen = true;
        }

        let modifiers = state.modifiers();
        let resolved: Option<Binding> = CONFIG.get().and_then(|config| {
            config
                .read()
                .ok()
                .and_then(|config| config.resolve(modifiers))
        });

        let previous = state.active;
        let next = resolved.map(|binding| binding.gesture);

        if next != previous {
            if let Some(gesture) = previous {
                emit(HotkeyEvent::Released(gesture));
            }
            state.active = next;
            if let Some(binding) = resolved {
                if binding.win {
                    state.swallow_win_release = true;
                }
            }
            if let Some(gesture) = next {
                emit(HotkeyEvent::Pressed(gesture));
            }
        }

        let mut swallow = false;
        if !down && is_win_key {
            if state.swallow_win_release && !state.other_key_seen {
                // Stop the Start menu from opening on the Win key release.
                swallow = true;
            }
            state.swallow_win_release = false;
            state.other_key_seen = false;
        }

        let snapshot = HotkeySnapshot {
            gesture: state.active,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
            win: modifiers.win,
        };
        if state.last_sent != snapshot {
            state.last_sent = snapshot;
            emit(HotkeyEvent::State(snapshot));
        }

        swallow
    }

    fn emit(event: HotkeyEvent) {
        if let Some(sender) = SENDER.get() {
            // A closed channel only means the app is shutting down.
            let _ = sender.send(event);
        }
    }
}
