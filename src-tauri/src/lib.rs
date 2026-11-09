//! Murmur — hold-to-talk voice assistant for Windows.
//!
//! Three gestures, all hold-to-talk:
//!   * Ctrl+Win    → dictation into the focused app
//!   * Ctrl+Shift  → circle-to-ask about what is on screen
//!   * Ctrl+Alt    → schedule a task by voice
//!
//! Voice recordings and circle-to-ask screenshots live in memory only.
//! Clipboard history is separately opt-in and stored locally in the webview;
//! the API key lives in Windows Credential Manager.

mod actions;
mod app_icons;
mod audio;
mod control;
mod dashboard;
mod db;
mod gemini;
mod hotkeys;
mod loadout;
mod notch;
mod overlay;
mod scheduler;
mod screen;
mod state;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Emitter, Manager};
use windows::core::w;
use windows::Devices::Radios::RadioKind;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HANDLE, WIN32_ERROR,
};
use windows::Win32::System::Threading::CreateMutexW;

use audio::AudioClip;
use hotkeys::{Gesture, HotkeyConfig, HotkeyEvent};
use loadout::Loadout;
use notch::NotchState;
use state::{AppState, CircleConversation, CircleSession};

/// How long a follow-up may reuse the previous circle conversation.
const CIRCLE_FOLLOW_UP: Duration = Duration::from_secs(30);
const MAX_CIRCLE_HISTORY_TURNS: usize = 4;
/// How long the overlay waits for a loop after the keys are released.
const CIRCLE_ABANDON: Duration = Duration::from_secs(20);
/// The crop must be bigger than this, or the loop was a stray click.
const CIRCLE_MIN_SIDE: u32 = 24;
static LOADOUT_WRITE: Mutex<()> = Mutex::new(());

struct InstanceMutex(HANDLE);

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        if let Err(err) = unsafe { CloseHandle(self.0) } {
            eprintln!("[murmur] could not close the single-instance mutex: {err}");
        }
    }
}

pub fn run() {
    unsafe { SetLastError(WIN32_ERROR(0)) };
    let instance_mutex = match unsafe {
        CreateMutexW(None, false, w!("Local\\Murmur.Desktop.SingleInstance"))
    } {
        Ok(handle) if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS => InstanceMutex(handle),
        Ok(handle) => {
            if let Err(err) = unsafe { CloseHandle(handle) } {
                eprintln!("[murmur] could not close the duplicate-instance mutex: {err}");
            }
            eprintln!("[murmur] another instance is already running");
            return;
        }
        Err(err) => {
            eprintln!("[murmur] could not acquire the single-instance mutex: {err}");
            return;
        }
    };
    let _instance_mutex = instance_mutex;

    // Physical pixels everywhere before any window exists.
    screen::ensure_dpi_awareness();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            notch::restore_taskbar();

            // SQLite lives in %APPDATA%\com.murmur.desktop.
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let database = db::Database::open(data_dir.join("murmur.sqlite3")).map_err(into_box)?;
            let saved_loadout = database.load_loadout().map_err(into_box)?;
            app.manage(AppState::new(database));

            let meter = app.state::<AppState>().meter.clone();
            audio::spawn_meter(handle.clone(), meter);

            notch::create(&handle).map_err(into_box)?;
            // The resting mark is always on screen: 40 px, click-through.
            notch::show(&handle, NotchState::Idle, None).map_err(into_box)?;
            scheduler::spawn(handle.clone());

            if let Err(err) = spawn_hotkey_pipeline(handle.clone(), saved_loadout.active_bindings())
            {
                eprintln!("[murmur] hotkeys unavailable: {err:#}");
            }

            eprintln!(
                "[murmur] ready (gemini key {})",
                if gemini::has_api_key() {
                    "configured"
                } else {
                    "missing — add one in the app"
                }
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api_key_status,
            save_api_key,
            clear_api_key,
            list_tasks,
            update_task,
            pause_task,
            resume_task,
            cancel_task,
            delete_task,
            notch_demo,
            notch_hide,
            notch_copy,
            submit_circle,
            cancel_circle,
            start_dictation,
            stop_dictation,
            hotkey_bindings,
            mic_status,
            get_loadout,
            save_loadout,
            patch_loadout,
            export_loadout,
            import_loadout,
            control_snapshot,
            dashboard_stats,
            dashboard_resize_panel,
            dashboard_speed_test,
            dashboard_clipboard,
            dashboard_copy_image,
            dashboard_pick_files,
            dashboard_open_path,
            dashboard_drag_path,
            open_control_centre,
            open_notch_peek,
            close_notch_peek,
            open_launcher,
            close_control_centre,
            media_toggle,
            media_next,
            media_previous,
            set_volume,
            set_muted,
            set_radio_enabled,
            set_dark_mode,
            set_wallpaper,
            choose_wallpaper,
            launch_pinned_app,
            list_installed_apps,
            launch_installed_app,
            power_action,
            show_main_window,
        ]);

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("[murmur] failed to start: {err}");
            return;
        }
    };

    app.run(|_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            hotkeys::stop();
        }
    });
}

/// The setup hook returns `Box<dyn Error>`, but `anyhow::Error` deliberately
/// does not implement `std::error::Error`, so it needs an explicit conversion.
fn into_box(err: anyhow::Error) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(format!("{err:#}")))
}

fn spawn_hotkey_pipeline(app: AppHandle, bindings: Vec<hotkeys::Binding>) -> Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();
    hotkeys::spawn(HotkeyConfig::new(bindings), sender)?;

    tauri::async_runtime::spawn(async move {
        let mut tracker = hotkeys::GestureTracker::default();
        let mut request: Option<tokio::task::JoinHandle<()>> = None;
        while let Some(event) = receiver.recv().await {
            let busy = request.as_ref().is_some_and(|task| !task.is_finished())
                || app.state::<AppState>().gesture.try_lock()
                    .map_or(true, |owner| owner.is_some());
            let Some(event) = tracker.filter(event, hold_to_talk(&app), busy) else {
                continue;
            };
            match event {
                HotkeyEvent::State(snapshot) => {
                    if let Err(err) = app.emit(hotkeys::STATE_EVENT, snapshot) {
                        eprintln!("[murmur] could not emit the hotkey state: {err}");
                    }
                }
                HotkeyEvent::Pressed(gesture) => {
                    if notch::panels_open() {
                        let _ = notch::set_control_open(&app, false);
                    }
                    if let Err(err) = on_press(&app, gesture) {
                        eprintln!(
                            "[murmur] {} could not start recording: {err:#}",
                            gesture.label()
                        );
                        let _ = report_error(&app, &format!("{err:#}"), 2600);
                    }
                }
                HotkeyEvent::Released(gesture) => {
                    let handle = app.clone();
                    request = Some(tokio::spawn(async move {
                        if let Err(err) = on_release(&handle, gesture).await {
                            eprintln!(
                                "[murmur] gesture {} failed: {err:#}",
                                gesture.listening_label()
                            );
                            let _ = report_error(&handle, &format!("{err:#}"), 2600);
                        }
                    }));
                }
            }
        }
    });

    Ok(())
}

/// Every gesture is voice-driven, so a press always opens a recording session.
fn on_press(app: &AppHandle, gesture: Gesture) -> Result<()> {
    let state = app.state::<AppState>();
    let mut owner = state.gesture.try_lock()
        .map_err(|_| anyhow!("A voice request is already running"))?;
    if owner.is_some() {
        return Err(anyhow!("A voice gesture is already active"));
    }
    if let Err(err) = start_gesture(app, gesture) {
        let _ = stop_recording(app);
        if gesture == Gesture::CircleToAsk {
            overlay::close(app);
            let _ = take_circle(app);
        }
        return Err(err);
    }
    *owner = Some(gesture);
    Ok(())
}

fn start_gesture(app: &AppHandle, gesture: Gesture) -> Result<()> {
    if gesture == Gesture::CircleToAsk {
        return start_circle(app);
    }

    if gesture == Gesture::Dictation {
        let target = actions::foreground_window()
            .ok_or_else(|| anyhow!("No active window for dictation"))?;
        *app.state::<AppState>()
            .dictation_target
            .lock()
            .map_err(|_| anyhow!("dictation target lock poisoned"))? = Some(target);
    }
    start_recording(app)?;

    notch::show(app, NotchState::Listening, Some(gesture.listening_label()))?;
    Ok(())
}

fn report_error(app: &AppHandle, message: &str, hold_ms: u64) -> Result<()> {
    notch::show(app, NotchState::Error, Some(message))?;
    notch::idle_later(app.clone(), Duration::from_millis(hold_ms));
    Ok(())
}

fn start_recording(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let mut recorder = state
        .recorder
        .lock()
        .map_err(|_| anyhow!("the recorder lock is poisoned"))?;
    recorder.start()
}

/// The Loadout preference. A missing or unreadable row keeps the default
/// hold-to-talk behaviour.
fn hold_to_talk(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .db
        .load_loadout()
        .map(|loadout| loadout.hotkeys.hold_to_talk)
        .unwrap_or(true)
}

/// Stops the microphone if it is running. `Ok(None)` means it never started.
fn stop_recording(app: &AppHandle) -> Result<Option<AudioClip>> {
    let state = app.state::<AppState>();
    let mut recorder = state
        .recorder
        .lock()
        .map_err(|_| anyhow!("the recorder lock is poisoned"))?;

    if !recorder.is_recording() {
        return Ok(None);
    }

    Ok(Some(recorder.stop()?))
}

async fn on_release(app: &AppHandle, gesture: Gesture) -> Result<()> {
    let state = app.state::<AppState>();
    let mut owner = state.gesture.lock().await;
    if *owner != Some(gesture) {
        return Ok(());
    }
    let result = finish_recording_gesture(app, gesture).await;
    if result.is_err() && gesture == Gesture::CircleToAsk {
        overlay::close(app);
        let _ = take_circle(app);
    }
    if gesture != Gesture::CircleToAsk
        || state.circle.lock().map_or(true, |circle| circle.is_none())
    {
        *owner = None;
    }
    result
}

async fn finish_recording_gesture(app: &AppHandle, gesture: Gesture) -> Result<()> {
    let clip = match stop_recording(app) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("[murmur] recording could not be completed: {err:#}");
            if gesture == Gesture::CircleToAsk {
                overlay::close(app);
                let _ = take_circle(app);
            }
            report_error(app, &format!("{err:#}"), 2600)?;
            return Ok(());
        }
    };

    if gesture == Gesture::CircleToAsk {
        return finish_or_wait_for_circle(app, clip).await;
    }

    let Some(clip) = clip else {
        let _ = notch::idle(app);
        return Ok(());
    };

    if clip.duration_ms < 250 {
        notch::show(
            app,
            NotchState::Error,
            Some("Hold the keys while you speak"),
        )?;
        notch::idle_later(app.clone(), Duration::from_millis(1800));
        return Ok(());
    }

    let wav = clip.to_wav();

    match gesture {
        Gesture::Dictation => {
            let target = app
                .state::<AppState>()
                .dictation_target
                .lock()
                .map_err(|_| anyhow!("dictation target lock poisoned"))?
                .take()
                .ok_or_else(|| anyhow!("No active window for dictation"))?;
            notch::update(app, NotchState::Thinking, Some("Transcribing…"))?;
            let client = gemini::GeminiClient::from_stored_key()?;
            let voice = app
                .state::<AppState>()
                .db
                .load_loadout()
                .map(|loadout| loadout.voice)
                .unwrap_or_default();
            let transcript = client.transcribe(wav, &voice).await?;

            if transcript.is_empty() {
                notch::update(app, NotchState::Error, Some("Nothing was transcribed"))?;
                notch::idle_later(app.clone(), Duration::from_millis(1800));
                return Ok(());
            }

            // The clipboard round-trip sleeps, so keep it off the runtime.
            notch::update(app, NotchState::Thinking, Some("Inserting…"))?;
            let to_insert = transcript.clone();
            let inserted = tauri::async_runtime::spawn_blocking(move || {
                actions::insert_text_for_window(&to_insert, Some(target))
            })
            .await?;
            if let Err(err) = inserted {
                notch::update(
                    app,
                    NotchState::Answer,
                    Some(&format!(
                        "{err}. Copy your transcript below:\n\n{transcript}"
                    )),
                )?;
                return Ok(());
            }

            notch::update(app, NotchState::Answer, Some(&transcript))?;
            notch::idle_later(app.clone(), Duration::from_millis(1400));
            Ok(())
        }
        Gesture::CircleToAsk => {
            // Unreachable: circle-to-ask is diverted before this match because it
            // needs the overlay's selection as well as the audio.
            Ok(())
        }
        Gesture::ScheduleTask => {
            notch::update(app, NotchState::Thinking, Some("Scheduling…"))?;
            let client = gemini::GeminiClient::from_stored_key()?;
            let planned = client.plan_task(wav).await?;

            let task = {
                let state = app.state::<AppState>();
                let fields = scheduler::task_fields(
                    &planned.prompt,
                    planned.action,
                    &planned.payload,
                    planned.recurrence.as_deref(),
                    planned.due_at_ms,
                )?;
                state.db.insert_task(&fields)?
            };

            let _ = app.emit(scheduler::TASK_DUE_EVENT, &task);
            let detail = match task.schedule {
                scheduler::ScheduleKind::Recurring => format!(
                    "Scheduled: {} (repeats {})",
                    task.prompt,
                    task.recurrence.as_deref().unwrap_or("recurring")
                ),
                scheduler::ScheduleKind::OneTime => format!("Scheduled: {}", task.prompt),
            };
            notch::update(app, NotchState::Answer, Some(&detail))?;
            notch::idle_later(app.clone(), Duration::from_millis(2200));
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Circle to ask
// ---------------------------------------------------------------------------

/// Captures the screen first, then opens the overlay and starts the mic. The
/// order matters: the frame must not contain the overlay.
fn start_circle(app: &AppHandle) -> Result<()> {
    {
        let state = app.state::<AppState>();
        let guard = state
            .circle
            .lock()
            .map_err(|_| anyhow!("the circle state is poisoned"))?;
        if guard.is_some() {
            return Ok(());
        }
    }

    // Clear the pill before grabbing the frame: a previous answer card would
    // otherwise be part of the screenshot. One frame of settle time is enough
    // for the compositor to drop it.
    let _ = notch::idle(app);
    std::thread::sleep(Duration::from_millis(60));

    let monitor = screen::monitor_under_cursor()?;
    let screenshot = screen::capture(monitor)?;

    {
        let state = app.state::<AppState>();
        let mut guard = state
            .circle
            .lock()
            .map_err(|_| anyhow!("the circle state is poisoned"))?;
        *guard = Some(CircleSession {
            started: Instant::now(),
            monitor,
            screenshot,
            selection: None,
            clip: None,
        });
    }

    if let Err(err) = overlay::show(app, monitor) {
        let _ = take_circle(app);
        return Err(err);
    }

    if let Err(err) = start_recording(app) {
        overlay::close(app);
        let _ = take_circle(app);
        return Err(err);
    }

    // The overlay covers the notch on this monitor; keep the pill quiet.
    let _ = notch::idle(app);
    Ok(())
}

fn take_circle(app: &AppHandle) -> Option<CircleSession> {
    let state = app.state::<AppState>();
    let session = match state.circle.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => {
            eprintln!("[murmur] the circle state is poisoned");
            None
        }
    };
    session
}

/// Called when the keys are released. Sends as soon as a loop exists, otherwise
/// waits for the loop while the overlay stays up.
async fn finish_or_wait_for_circle(app: &AppHandle, clip: Option<AudioClip>) -> Result<()> {
    let ready = {
        let state = app.state::<AppState>();
        let mut guard = state
            .circle
            .lock()
            .map_err(|_| anyhow!("the circle state is poisoned"))?;

        let complete = guard.as_mut().map(|session| {
            if clip.is_some() {
                session.clip = clip;
            }
            session.selection.is_some() && session.clip.is_some()
        });

        if complete == Some(true) {
            guard.take()
        } else {
            None
        }
    };

    if let Some(session) = ready {
        return finish_circle(app, session).await;
    }

    let started = {
        let state = app.state::<AppState>();
        let guard = state
            .circle
            .lock()
            .map_err(|_| anyhow!("the circle state is poisoned"))?;
        guard.as_ref().map(|session| session.started)
    };

    if let Some(started) = started {
        schedule_circle_timeout(app.clone(), started);
    }
    Ok(())
}

/// Abandons the gesture if no loop is drawn: the overlay must never stay stuck
/// on top of the user's screen.
fn schedule_circle_timeout(app: AppHandle, started: Instant) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CIRCLE_ABANDON).await;
        let state = app.state::<AppState>();
        let mut owner = state.gesture.lock().await;

        let abandoned = {
            let state = app.state::<AppState>();
            let Ok(mut guard) = state.circle.lock() else {
                return;
            };

            let stale = guard
                .as_ref()
                .map(|session| session.started == started && session.selection.is_none())
                .unwrap_or(false);

            if stale {
                guard.take()
            } else {
                None
            }
        };

        if abandoned.is_some() {
            *owner = None;
            overlay::close(&app);
            let _ = notch::idle(&app);
        }
    });
}

async fn finish_circle(app: &AppHandle, session: CircleSession) -> Result<()> {
    overlay::close(app);

    let Some(selection) = session.selection else {
        return Ok(());
    };
    let Some(clip) = session.clip else {
        return Ok(());
    };

    if clip.duration_ms < 250 {
        return report_error(app, "Hold the keys while you ask", 2200);
    }

    // Normalised selection -> physical pixels -> padded, clamped crop.
    let rect = screen::crop_rect(selection, session.monitor, screen::PADDING);
    if rect.is_tiny(CIRCLE_MIN_SIDE) {
        return report_error(app, "Circle something a bit larger", 2400);
    }

    notch::show(app, NotchState::Thinking, Some("Looking at your screen…"))?;

    let crop = session.screenshot.crop(rect);
    let full = session
        .screenshot
        .downscaled_to_width(screen::CONTEXT_WIDTH);

    // PNG encoding is CPU-bound; keep it off the async runtime.
    let (crop_png, full_png) = tauri::async_runtime::spawn_blocking(move || {
        let crop_png = crop.to_png()?;
        let full_png = full.to_png()?;
        Ok::<_, anyhow::Error>((crop_png, full_png))
    })
    .await??;

    let wav = clip.to_wav();
    let user = gemini::Content::user(vec![
        gemini::Part::text("Circled region, cropped:"),
        gemini::Part::inline("image/png", &crop_png),
        gemini::Part::text("Full screen, downscaled, for context:"),
        gemini::Part::inline("image/png", &full_png),
        gemini::Part::inline("audio/wav", &wav),
        gemini::Part::text("Answer the spoken question about the circled region."),
    ]);

    let mut contents = recent_turns(app)?;
    contents.push(user.clone());

    let persona = app
        .state::<AppState>()
        .db
        .load_loadout()
        .map(|loadout| loadout.persona)
        .unwrap_or_default();

    let client = gemini::GeminiClient::from_stored_key()?;
    let answer = client.answer_about_circle(contents, &persona).await?;

    remember_turn(app, user, &answer)?;

    notch::show(app, NotchState::Answer, Some(&answer))?;
    // The card dismisses itself after 20 s; this is only a backstop past the
    // 30 s follow-up window.
    notch::idle_later(app.clone(), Duration::from_secs(35));
    Ok(())
}

fn recent_turns(app: &AppHandle) -> Result<Vec<gemini::Content>> {
    let state = app.state::<AppState>();
    let mut guard = state
        .conversation
        .lock()
        .map_err(|_| anyhow!("the conversation state is poisoned"))?;

    clear_expired_circle_history(&mut guard);
    match guard.as_ref() {
        Some(conversation) if conversation.updated_at.elapsed() < CIRCLE_FOLLOW_UP => {
            Ok(conversation.turns.clone())
        }
        _ => Ok(Vec::new()),
    }
}

fn remember_turn(app: &AppHandle, user: gemini::Content, answer: &str) -> Result<()> {
    let updated_at = {
        let state = app.state::<AppState>();
        let mut guard = state
            .conversation
            .lock()
            .map_err(|_| anyhow!("the conversation state is poisoned"))?;

        clear_expired_circle_history(&mut guard);
        let conversation = guard.get_or_insert_with(|| CircleConversation {
            turns: Vec::new(),
            updated_at: Instant::now(),
        });

        conversation.turns.push(user);
        conversation
            .turns
            .push(gemini::Content::model(answer.to_string()));
        trim_circle_history(&mut conversation.turns);
        conversation.updated_at = Instant::now();
        conversation.updated_at
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CIRCLE_FOLLOW_UP).await;
        let state = app.state::<AppState>();
        match state.conversation.lock() {
            Ok(mut guard) => {
                clear_circle_history_if_unchanged(&mut guard, updated_at);
            }
            Err(err) => eprintln!("[murmur] the conversation state is poisoned: {err}"),
        };
    });
    Ok(())
}

fn clear_expired_circle_history(conversation: &mut Option<CircleConversation>) {
    if conversation
        .as_ref()
        .is_some_and(|turns| turns.updated_at.elapsed() >= CIRCLE_FOLLOW_UP)
    {
        *conversation = None;
    }
}

fn clear_circle_history_if_unchanged(
    conversation: &mut Option<CircleConversation>,
    updated_at: Instant,
) {
    if conversation
        .as_ref()
        .is_some_and(|turns| turns.updated_at == updated_at)
    {
        *conversation = None;
    }
}

fn trim_circle_history(turns: &mut Vec<gemini::Content>) {
    let excess = turns.len().saturating_sub(MAX_CIRCLE_HISTORY_TURNS);
    turns.drain(..excess);
}

#[cfg(test)]
mod circle_history_tests {
    use super::*;

    fn conversation(updated_at: Instant) -> CircleConversation {
        CircleConversation {
            turns: vec![gemini::Content::model("screen/audio history")],
            updated_at,
        }
    }

    #[test]
    fn expired_circle_history_is_cleared() {
        let mut history = Some(conversation(
            Instant::now() - CIRCLE_FOLLOW_UP - Duration::from_secs(1),
        ));
        clear_expired_circle_history(&mut history);
        assert!(history.is_none());
    }

    #[test]
    fn expiry_timer_does_not_clear_newer_history() {
        let newer = Instant::now();
        let mut history = Some(conversation(newer));
        clear_circle_history_if_unchanged(&mut history, newer - Duration::from_secs(1));
        assert!(history.is_some());
        clear_circle_history_if_unchanged(&mut history, newer);
        assert!(history.is_none());
    }

    #[test]
    fn circle_history_keeps_only_the_latest_turns() {
        let mut turns: Vec<_> = (0..MAX_CIRCLE_HISTORY_TURNS + 2)
            .map(|index| gemini::Content::model(index.to_string()))
            .collect();
        trim_circle_history(&mut turns);
        assert_eq!(turns.len(), MAX_CIRCLE_HISTORY_TURNS);
        assert_eq!(
            turns
                .first()
                .and_then(|turn| turn.parts.first())
                .and_then(|part| part.text.as_deref()),
            Some("2")
        );
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn api_key_status() -> bool {
    gemini::has_api_key()
}

#[tauri::command]
fn save_api_key(key: String) -> Result<(), String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("The API key is empty.".into());
    }
    gemini::store_api_key(trimmed).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn clear_api_key() -> Result<(), String> {
    gemini::clear_api_key().map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn list_tasks(state: tauri::State<'_, AppState>) -> Result<Vec<scheduler::ScheduledTask>, String> {
    state.db.list_tasks(200).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn update_task(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    prompt: String,
    due_at_ms: i64,
    action: String,
    payload: String,
    recurrence: Option<String>,
) -> Result<scheduler::ScheduledTask, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("A task needs a prompt.".into());
    }

    let action = scheduler::ActionKind::parse(&action).map_err(|err| format!("{err:#}"))?;
    let recurrence = recurrence
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty());

    let now = db::now_ms();
    let due_at_ms = match recurrence.as_deref() {
        Some(rule) => {
            let parsed = scheduler::Recurrence::parse(rule).map_err(|err| format!("{err:#}"))?;
            if due_at_ms <= now {
                parsed
                    .next_after_ms(now)
                    .ok_or_else(|| "The recurrence rule has no next occurrence.".to_string())?
            } else {
                due_at_ms
            }
        }
        None if due_at_ms <= 0 => return Err("A task needs a due time.".into()),
        None => due_at_ms,
    };

    let payload = if payload.trim().is_empty() && action == scheduler::ActionKind::Remind {
        prompt.clone()
    } else {
        payload
    };

    let fields =
        scheduler::task_fields(&prompt, action, &payload, recurrence.as_deref(), due_at_ms)
            .map_err(|err| format!("{err:#}"))?;

    let task = state
        .db
        .update_task(id, &fields)
        .map_err(|err| format!("{err:#}"))?
        .ok_or_else(|| "Task not found.".to_string())?;

    let _ = app.emit(scheduler::TASK_DUE_EVENT, &task);
    Ok(task)
}

#[tauri::command]
fn pause_task(app: AppHandle, state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    let changed = state.db.pause_task(id).map_err(|err| format!("{err:#}"))?;
    emit_task(&app, &state, id);
    Ok(changed)
}

#[tauri::command]
fn resume_task(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<scheduler::ScheduledTask, String> {
    let existing = state
        .db
        .get_task(id)
        .map_err(|err| format!("{err:#}"))?
        .ok_or_else(|| "Task not found.".to_string())?;

    let due_at_ms = scheduler::due_after_resume(&existing, db::now_ms());
    let task = state
        .db
        .resume_task(id, Some(due_at_ms))
        .map_err(|err| format!("{err:#}"))?
        .ok_or_else(|| "Only a paused task can be resumed.".to_string())?;

    let _ = app.emit(scheduler::TASK_DUE_EVENT, &task);
    Ok(task)
}

#[tauri::command]
fn cancel_task(app: AppHandle, state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    let changed = state.db.cancel_task(id).map_err(|err| format!("{err:#}"))?;
    emit_task(&app, &state, id);
    Ok(changed)
}

#[tauri::command]
fn delete_task(app: AppHandle, state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    let changed = state.db.delete_task(id).map_err(|err| format!("{err:#}"))?;
    emit_task(&app, &state, id);
    Ok(changed)
}

/// Pushes the current row to the UI after a mutation. Deleted rows simply emit
/// nothing; the command result tells the caller to refresh.
fn emit_task(app: &AppHandle, state: &tauri::State<'_, AppState>, id: i64) {
    if let Ok(Some(task)) = state.db.get_task(id) {
        let _ = app.emit(scheduler::TASK_DUE_EVENT, &task);
    }
}

#[tauri::command]
fn hotkey_bindings() -> Vec<hotkeys::Binding> {
    hotkeys::current_bindings()
}

#[tauri::command]
fn mic_status() -> audio::MicInfo {
    audio::probe_default_input()
}

/// Markdown for the preview button; doubles as a check of the renderer.
const ANSWER_DEMO: &str = "**Drafted** a follow-up email to the design team for Friday.\n\n- recap the type scale\n- attach the new notch states\n\n`Ctrl+Win` starts dictation.";

#[tauri::command]
fn notch_demo(app: AppHandle, state: String) -> Result<(), String> {
    let parsed =
        NotchState::parse(&state).ok_or_else(|| format!("unknown notch state: {state}"))?;
    let text = match parsed {
        NotchState::Idle => None,
        NotchState::Listening => Some("Listening…"),
        NotchState::Thinking => Some("Thinking…"),
        NotchState::Answer => Some(ANSWER_DEMO),
        NotchState::Error => Some("Gemini API key is not set"),
    };
    notch::show(&app, parsed, text).map_err(|err| format!("{err:#}"))
}

/// Dismissing the card returns to the resting mark; the window stays visible
/// and click-through, so the assistant keeps a consistent anchor on screen.
#[tauri::command]
fn notch_hide(app: AppHandle) -> Result<(), String> {
    notch::idle(&app).map_err(|err| format!("{err:#}"))
}

/// Copy button on the answer card, through the same clipboard path dictation uses.
#[tauri::command]
fn notch_copy(text: String) -> Result<(), String> {
    actions::copy_text(&text).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn dashboard_stats() -> Result<dashboard::Stats, String> {
    blocking(|| Ok(dashboard::stats())).await
}

#[tauri::command]
fn dashboard_resize_panel(app: AppHandle, panel: String) -> Result<(), String> {
    notch::size_control_panel(&app, &panel).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn dashboard_speed_test() -> Result<dashboard::SpeedResult, String> {
    dashboard::speed_test()
        .await
        .map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn dashboard_clipboard() -> Result<dashboard::ClipboardItem, String> {
    blocking(dashboard::clipboard).await
}

#[tauri::command]
async fn dashboard_copy_image(data: String) -> Result<(), String> {
    blocking(move || dashboard::copy_image(&data)).await
}

#[tauri::command]
async fn dashboard_pick_files(folder: bool) -> Vec<String> {
    let dialog = rfd::AsyncFileDialog::new();
    if folder {
        dialog
            .pick_folder()
            .await
            .into_iter()
            .map(|p| p.path().to_string_lossy().into_owned())
            .collect()
    } else {
        dialog
            .pick_files()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.path().to_string_lossy().into_owned())
            .collect()
    }
}

#[tauri::command]
async fn dashboard_open_path(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("This item no longer exists.".into());
    }
    blocking(move || actions::open_app(&path)).await
}

#[tauri::command]
async fn dashboard_drag_path(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    let path = std::fs::canonicalize(path).map_err(|e| format!("Cannot open shelf item: {e}"))?;
    let drag_window = window.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .run_on_main_thread(move || {
            let image = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
            let options = drag::Options {
                mode: drag::DragMode::Copy,
                ..Default::default()
            };
            let result = drag::start_drag(
                &drag_window,
                drag::DragItem::Files(vec![path]),
                image,
                |_, _| {},
                options,
            )
            .map_err(|err| format!("Cannot start drag: {err}"));
            let _ = sender.send(result);
        })
        .map_err(|err| format!("Cannot start drag: {err}"))?;
    receiver
        .await
        .map_err(|_| "Drag was interrupted".to_string())?
}

/// The overlay hands over the finished loop. Closing the loop ends the
/// question, so the microphone stops here even if the keys are still held.
#[tauri::command]
async fn submit_circle(app: AppHandle, selection: screen::NormalizedRect) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut owner = state.gesture.lock().await;
    if *owner != Some(Gesture::CircleToAsk) {
        return Err("No screen question is active.".into());
    }
    *owner = None;
    let clip = match stop_recording(&app) {
        Ok(clip) => clip,
        Err(err) => {
            overlay::close(&app);
            let _ = take_circle(&app);
            let message = format!("Could not finish screen question: {err:#}");
            report_error(&app, &message, 3000).map_err(|report_err| format!("{report_err:#}"))?;
            return Err(message);
        }
    };
    overlay::close(&app);

    let ready = {
        let state = app.state::<AppState>();
        let mut guard = state
            .circle
            .lock()
            .map_err(|_| "the circle state is poisoned".to_string())?;

        let complete = guard.as_mut().map(|session| {
            session.selection = Some(selection);
            if clip.is_some() {
                session.clip = clip;
            }
            session.clip.is_some()
        });

        if complete == Some(true) {
            guard.take()
        } else {
            None
        }
    };

    let Some(session) = ready else {
        let _ = take_circle(&app);
        let message = "Could not finish screen question; start it again.";
        report_error(&app, message, 3000).map_err(|err| format!("{err:#}"))?;
        return Err(message.into());
    };

    if let Err(err) = finish_circle(&app, session).await {
        eprintln!("[murmur] circle request failed: {err:#}");
        let message = format!("{err:#}");
        report_error(&app, &message, 3000).map_err(|report_err| format!("{report_err:#}"))?;
        return Err(message);
    }

    Ok(())
}

#[tauri::command]
async fn cancel_circle(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut owner = state.gesture.lock().await;
    if *owner != Some(Gesture::CircleToAsk) {
        return Ok(());
    }
    *owner = None;
    overlay::close(&app);
    let _ = take_circle(&app);
    let _ = stop_recording(&app);
    notch::idle(&app).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn start_dictation(app: AppHandle) -> Result<(), String> {
    on_press(&app, Gesture::Dictation).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn stop_dictation(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut owner = state.gesture.lock().await;
    if *owner != Some(Gesture::Dictation) {
        return Err("No test dictation is active.".into());
    }
    *owner = None;
    let clip = {
        let state = app.state::<AppState>();
        let mut recorder = state
            .recorder
            .lock()
            .map_err(|_| "the recorder lock is poisoned".to_string())?;
        recorder.stop().map_err(|err| format!("{err:#}"))?
    };

    if clip.duration_ms < 250 {
        let _ = notch::show(&app, NotchState::Error, Some("Recording was too short"));
        notch::idle_later(app.clone(), Duration::from_millis(1600));
        return Err("Recording was too short to transcribe.".into());
    }

    notch::update(&app, NotchState::Thinking, Some("Transcribing…"))
        .map_err(|err| format!("{err:#}"))?;
    let result = async {
        let voice = {
            let state = app.state::<AppState>();
            state
                .db
                .load_loadout()
                .map(|loadout| loadout.voice)
                .unwrap_or_default()
        };
        let client = gemini::GeminiClient::from_stored_key().map_err(|err| format!("{err:#}"))?;
        client
            .transcribe(clip.to_wav(), &voice)
            .await
            .map_err(|err| format!("{err:#}"))
    }
    .await;

    match result {
        Ok(transcript) if transcript.is_empty() => {
            report_error(&app, "Nothing was transcribed", 1800)
                .map_err(|err| format!("{err:#}"))?;
            Err("Nothing was transcribed.".into())
        }
        Ok(transcript) => {
            notch::update(&app, NotchState::Answer, Some(&transcript))
                .map_err(|err| format!("{err:#}"))?;
            notch::idle_later(app.clone(), Duration::from_millis(1800));
            Ok(transcript)
        }
        Err(err) => {
            report_error(&app, &err, 2600).map_err(|report_err| format!("{report_err:#}"))?;
            Err(err)
        }
    }
}

// ---------------------------------------------------------------------------
// Loadout
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_loadout(state: tauri::State<'_, AppState>) -> Result<Loadout, String> {
    state.db.load_loadout().map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn save_loadout(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    loadout: Loadout,
) -> Result<Loadout, String> {
    apply_loadout(&app, &state.db, loadout)
}

#[tauri::command]
fn patch_loadout(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Loadout, String> {
    let _guard = LOADOUT_WRITE
        .lock()
        .map_err(|_| "Loadout lock poisoned".to_string())?;
    let current = state.db.load_loadout().map_err(|err| format!("{err:#}"))?;
    let loadout = merge_loadout_patch(current, patch)?;
    apply_loadout_unlocked(&app, &state.db, loadout)
}

fn merge_loadout_patch(current: Loadout, patch: serde_json::Value) -> Result<Loadout, String> {
    let mut current = serde_json::to_value(current).map_err(|err| err.to_string())?;
    let fields = patch.as_object().ok_or("Loadout patch must be an object")?;
    for (key, value) in fields {
        if !matches!(
            key.as_str(),
            "voice" | "hotkeys" | "persona" | "apps" | "theme"
        ) {
            return Err(format!("Unknown Loadout field: {key}"));
        }
        current[key] = value.clone();
    }
    serde_json::from_value(current).map_err(|err| format!("Invalid Loadout patch: {err}"))
}

#[cfg(test)]
mod loadout_patch_tests {
    use super::*;

    #[test]
    fn patch_preserves_other_sections_and_rejects_unknown_fields() {
        let mut current = Loadout::default();
        current.voice.custom = "Keep this".into();
        let updated = merge_loadout_patch(
            current.clone(),
            serde_json::json!({"apps": {"pinned": ["Example"]}}),
        )
        .unwrap();
        assert_eq!(updated.voice, current.voice);
        assert_eq!(updated.apps.pinned, vec!["Example"]);
        assert!(merge_loadout_patch(current, serde_json::json!({"unknown": true})).is_err());
    }
}

#[tauri::command]
fn export_loadout(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let loadout = state.db.load_loadout().map_err(|err| format!("{err:#}"))?;
    serde_json::to_string_pretty(&loadout)
        .map_err(|err| format!("could not serialize the loadout: {err}"))
}

#[tauri::command]
fn import_loadout(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    json: String,
) -> Result<Loadout, String> {
    let loadout: Loadout = serde_json::from_str(&json)
        .map_err(|err| format!("That file is not a Murmur loadout: {err}"))?;
    apply_loadout(&app, &state.db, loadout)
}

fn apply_loadout(app: &AppHandle, db: &db::Database, loadout: Loadout) -> Result<Loadout, String> {
    let _guard = LOADOUT_WRITE
        .lock()
        .map_err(|_| "Loadout lock poisoned".to_string())?;
    apply_loadout_unlocked(app, db, loadout)
}

fn apply_loadout_unlocked(
    app: &AppHandle,
    db: &db::Database,
    loadout: Loadout,
) -> Result<Loadout, String> {
    let saved = db
        .save_loadout(&loadout)
        .map_err(|err| format!("{err:#}"))?;
    if let Err(err) = hotkeys::set_bindings(saved.active_bindings()) {
        eprintln!("[murmur] could not apply the new hotkey bindings: {err:#}");
    }
    // The notch window holds its own copy of the Loadout for its theme and
    // wake behaviour, so a save has to reach it as well as the settings window.
    if let Err(err) = app.emit(loadout::CHANGED_EVENT, &saved) {
        eprintln!("[murmur] could not broadcast the loadout: {err}");
    }
    Ok(saved)
}

// ---------------------------------------------------------------------------
// Control Centre
// ---------------------------------------------------------------------------

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| format!("background task failed: {err}"))?
        .map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn control_snapshot() -> Result<control::ControlSnapshot, String> {
    blocking(|| Ok(control::ControlSnapshot::read())).await
}

#[tauri::command]
fn open_control_centre(app: AppHandle) -> Result<(), String> {
    notch::set_control_open(&app, true).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn open_notch_peek(app: AppHandle) -> Result<(), String> {
    notch::set_peek_open(&app, true).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn close_notch_peek(app: AppHandle) -> Result<(), String> {
    notch::set_peek_open(&app, false).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn open_launcher(app: AppHandle) -> Result<(), String> {
    notch::set_launcher_open(&app).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn close_control_centre(app: AppHandle) -> Result<(), String> {
    notch::set_control_open(&app, false).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
async fn media_toggle() -> Result<(), String> {
    blocking(control::media_toggle).await
}

#[tauri::command]
async fn media_next() -> Result<(), String> {
    blocking(control::media_next).await
}

#[tauri::command]
async fn media_previous() -> Result<(), String> {
    blocking(control::media_previous).await
}

#[tauri::command]
async fn set_volume(level: f64) -> Result<(), String> {
    blocking(move || control::set_volume(level)).await
}

#[tauri::command]
async fn set_muted(muted: bool) -> Result<(), String> {
    blocking(move || control::set_muted(muted)).await
}

#[tauri::command]
async fn set_radio_enabled(kind: String, enabled: bool) -> Result<(), String> {
    let radio = match kind.as_str() {
        "wifi" => RadioKind::WiFi,
        "bluetooth" => RadioKind::Bluetooth,
        other => return Err(format!("Unknown radio: {other}")),
    };
    blocking(move || control::set_radio(radio, enabled)).await
}

#[tauri::command]
async fn set_dark_mode(enabled: bool) -> Result<(), String> {
    blocking(move || control::set_dark_mode(enabled)).await
}

#[tauri::command]
async fn set_wallpaper(app: AppHandle, preset: String) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("Find Murmur's data directory: {err}"))?;
    blocking(move || control::set_wallpaper(&data_dir, &preset)).await
}

#[tauri::command]
async fn choose_wallpaper() -> Result<Option<String>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Images", &["jpg", "jpeg", "png", "bmp"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    let display = path.to_string_lossy().into_owned();
    blocking(move || control::set_wallpaper_file(&path)).await?;
    Ok(Some(display))
}

#[tauri::command]
async fn launch_pinned_app(name: String) -> Result<(), String> {
    blocking(move || actions::open_app(&name)).await
}

#[tauri::command]
async fn list_installed_apps() -> Result<Vec<actions::InstalledApp>, String> {
    blocking(actions::installed_apps).await
}

#[tauri::command]
async fn launch_installed_app(target: String) -> Result<(), String> {
    blocking(move || actions::launch_installed_app(&target)).await
}

#[tauri::command]
async fn power_action(action: String) -> Result<(), String> {
    blocking(move || control::power_action(&action)).await
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|err| format!("{err}"))?;
        let _ = window.unminimize();
        window.set_focus().map_err(|err| format!("{err}"))?;
    }
    let _ = app.emit_to("main", "app://loadout-focus", ());
    Ok(())
}
