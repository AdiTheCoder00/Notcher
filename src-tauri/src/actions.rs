//! Operating-system side effects for scheduled tasks: launching apps, opening
//! URLs, searching the web, Windows toast reminders, and typing text.
//!
//! App names are resolved the way Windows itself resolves them: the App Paths
//! registry keys first, then Start Menu shortcuts. A shortcut is launched
//! through the shell, which also resolves its target, arguments and working
//! directory exactly as clicking it would.

use std::env;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use url::Url;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winreg::RegKey;

use crate::scheduler::ActionKind;

/// Give the rest of the system a moment to see the new clipboard contents
/// before the paste shortcut fires.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(40);
/// How long the dictated text stays on the clipboard before the old one returns.
const RESTORE_AFTER: Duration = Duration::from_millis(300);
const APP_PATHS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";

/// Runs one scheduled task action. Called on a blocking thread by the
/// scheduler because typing and process launch can take a short while.
pub fn execute(app: &AppHandle, action: ActionKind, payload: &str, prompt: &str) -> Result<()> {
    match action {
        ActionKind::OpenApp => open_app(text_or_prompt(payload, prompt)),
        ActionKind::OpenUrl => open_url(text_or_prompt(payload, prompt)),
        ActionKind::RunSearch => run_search(text_or_prompt(payload, prompt)),
        ActionKind::TypeText => insert_text(text_or_prompt(payload, prompt)),
        ActionKind::Remind => {
            let body = text_or_prompt(payload, prompt);
            app.notification()
                .builder()
                .title("Murmur reminder")
                .body(body)
                .show()
                .map_err(|err| anyhow!("could not show the Windows notification: {err}"))
        }
    }
}

fn text_or_prompt<'a>(payload: &'a str, prompt: &'a str) -> &'a str {
    if payload.trim().is_empty() {
        prompt
    } else {
        payload
    }
}

// ---------------------------------------------------------------------------
// open_app
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct FoundApp {
    score: i32,
    target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub target: String,
}

pub fn installed_apps() -> Result<Vec<InstalledApp>> {
    let mut shortcuts = Vec::new();
    for dir in start_menu_dirs() {
        collect_shortcuts(&dir, 0, &mut shortcuts);
    }

    shortcuts.sort_by(|left, right| {
        left.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .cmp(
                &right
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
            )
    });

    let mut names = std::collections::HashSet::new();
    Ok(shortcuts
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().trim().to_string();
            if name.is_empty() || !names.insert(name.to_lowercase()) {
                return None;
            }
            Some(InstalledApp {
                name,
                target: path.to_string_lossy().into_owned(),
            })
        })
        .collect())
}

pub fn launch_installed_app(target: &str) -> Result<()> {
    let app = installed_apps()?
        .into_iter()
        .find(|app| app.target.eq_ignore_ascii_case(target))
        .ok_or_else(|| anyhow!("that app is no longer available in the Start Menu"))?;
    launch_shell(&app.target)
}

/// Resolves and launches an application by name. The target may be an
/// executable from App Paths, a Start Menu `.lnk`, or a literal path the user
/// gave us.
pub fn open_app(app_name: &str) -> Result<()> {
    let query = app_name.trim();
    if query.is_empty() {
        return Err(anyhow!("the task did not say which app to open"));
    }

    if Path::new(query).is_file() {
        return launch_shell(query);
    }

    let mut found = app_path_candidates(query);
    found.extend(start_menu_candidates(query));
    found.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.target.cmp(&right.target))
    });

    if let Some(best) = found.first() {
        return launch_shell(&best.target);
    }

    // Last resort: let the shell resolve the bare name through its own App
    // Paths/PATH lookup instead of failing outright.
    launch_shell(query)
}

fn app_path_candidates(query: &str) -> Vec<FoundApp> {
    let mut found = Vec::new();

    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let root = RegKey::predef(hive);
            let flags = KEY_READ | view;
            let Ok(app_paths) = root.open_subkey_with_flags(APP_PATHS_KEY, flags) else {
                continue;
            };

            for name in app_paths.enum_keys().flatten() {
                let Some(score) = score_app(query, &name) else {
                    continue;
                };
                let Ok(entry) = app_paths.open_subkey_with_flags(&name, flags) else {
                    continue;
                };
                let Ok(target) = entry.get_value::<String, _>("") else {
                    continue;
                };
                let target = target.trim().trim_matches('"').to_string();
                if !target.is_empty() {
                    found.push(FoundApp { score, target });
                }
            }
        }
    }

    found
}

fn start_menu_candidates(query: &str) -> Vec<FoundApp> {
    let mut shortcuts = Vec::new();
    for dir in start_menu_dirs() {
        collect_shortcuts(&dir, 0, &mut shortcuts);
    }

    shortcuts
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().to_string();
            let score = score_app(query, &name)?;
            Some(FoundApp {
                // Prefer App Paths on an exact tie: it launches the executable
                // directly rather than going through a shortcut.
                score: score - 1,
                target: path.to_string_lossy().to_string(),
            })
        })
        .collect()
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(appdata) = env::var_os("APPDATA") {
        dirs.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(programdata) = env::var_os("PROGRAMDATA") {
        dirs.push(
            PathBuf::from(programdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    dirs
}

fn collect_shortcuts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_shortcuts(&path, depth + 1, out);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn score_app(query: &str, name: &str) -> Option<i32> {
    let query = normalise_app_name(query);
    if query.is_empty() {
        return None;
    }
    let name = normalise_app_name(name);

    if name == query {
        Some(100)
    } else if name.starts_with(&query) {
        let extra = name.len().saturating_sub(query.len()) as i32;
        Some(80 - extra.min(20))
    } else if name.contains(&query) {
        Some(50)
    } else if query
        .split_whitespace()
        .all(|word| name.split_whitespace().any(|part| part == word))
    {
        Some(30)
    } else {
        None
    }
}

fn normalise_app_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .trim_end_matches(".lnk")
        .trim_end_matches(".exe")
        .replace('_', " ")
        .replace('-', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// open_url and run_search
// ---------------------------------------------------------------------------

pub fn open_url(raw: &str) -> Result<()> {
    let url =
        Url::parse(raw.trim()).map_err(|err| anyhow!("that does not look like a URL ({err})"))?;

    match url.scheme() {
        "http" | "https" => launch_shell(url.as_str()),
        other => Err(anyhow!(
            "only http and https URLs can be opened, not {other}"
        )),
    }
}

pub fn run_search(query: &str) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("the search query is empty"));
    }

    let mut url = Url::parse("https://www.google.com/search")
        .map_err(|err| anyhow!("could not build the search URL: {err}"))?;
    url.query_pairs_mut().append_pair("q", query);
    open_url(url.as_str())
}

fn launch_shell(target: &str) -> Result<()> {
    if target.trim().is_empty() {
        return Err(anyhow!("there is nothing to open"));
    }

    let target: Vec<u16> = std::ffi::OsStr::new(target)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR::from_raw(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        return Err(anyhow!(
            "could not launch the requested app or URL (Windows error {})",
            result.0 as usize
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Text insertion
// ---------------------------------------------------------------------------

/// Inserts text at the current focus. The notch never takes focus, so this
/// lands in the app the user was already in.
pub fn insert_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    match paste_via_clipboard(text) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Clipboard contention or a blocked shortcut: typing still works.
            eprintln!("[actions] clipboard paste failed ({err:#}); typing the text instead");
            type_directly(text)
        }
    }
}

fn paste_via_clipboard(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().map_err(|err| anyhow!("open the clipboard: {err}"))?;
    let previous = clipboard
        .get_text()
        .map_err(|err| anyhow!("read the clipboard before pasting: {err}"))?;

    clipboard
        .set_text(text.to_string())
        .map_err(|err| anyhow!("write the text to the clipboard: {err}"))?;
    thread::sleep(CLIPBOARD_SETTLE);

    if let Err(err) = paste_shortcut() {
        restore_if_unchanged(&mut clipboard, &previous, text);
        return Err(err);
    }

    thread::sleep(RESTORE_AFTER);
    restore_if_unchanged(&mut clipboard, &previous, text);
    Ok(())
}

fn restore_if_unchanged(clipboard: &mut Clipboard, previous: &str, inserted: &str) {
    let Ok(current) = clipboard.get_text() else {
        return;
    };
    if should_restore_clipboard(&current, inserted) {
        if let Err(err) = clipboard.set_text(previous.to_string()) {
            eprintln!("[actions] could not restore the previous clipboard text: {err}");
        }
    }
}

fn should_restore_clipboard(current: &str, inserted: &str) -> bool {
    current == inserted
}

#[cfg(test)]
mod tests {
    use super::should_restore_clipboard;

    #[test]
    fn clipboard_restore_only_replaces_the_text_inserted_by_murmur() {
        assert!(should_restore_clipboard("transcript", "transcript"));
        assert!(!should_restore_clipboard("new copy", "transcript"));
    }
}

fn paste_shortcut() -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|err| anyhow!("input backend unavailable: {err}"))?;

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|err| anyhow!("could not hold Ctrl: {err}"))?;

    let pressed = enigo.key(Key::Unicode('v'), Direction::Click);
    let released = enigo.key(Key::Control, Direction::Release);

    // Release Ctrl before reporting failure, so a failed paste cannot leave the
    // modifier stuck down across the whole desktop.
    pressed.map_err(|err| anyhow!("could not press V: {err}"))?;
    released.map_err(|err| anyhow!("could not release Ctrl: {err}"))?;
    Ok(())
}

/// Puts text on the clipboard without pasting. Used by the answer card's copy
/// button: the webview cannot reach the Windows clipboard reliably.
pub fn copy_text(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().map_err(|err| anyhow!("open the clipboard: {err}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| anyhow!("write the text to the clipboard: {err}"))?;
    Ok(())
}

fn type_directly(text: &str) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|err| anyhow!("input backend unavailable: {err}"))?;
    enigo
        .text(text)
        .map_err(|err| anyhow!("text injection failed: {err}"))?;
    Ok(())
}
