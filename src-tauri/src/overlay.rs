//! The circle-to-ask overlay: one transparent, always-on-top window covering
//! exactly the monitor under the cursor.
//!
//! It is deliberately a separate window from the notch. The notch must never
//! take focus (dictation pastes into the focused app), while this window needs
//! the pointer and the Escape key.

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::screen::MonitorRect;

pub const LABEL: &str = "capture";

/// Creates and shows the overlay over `monitor` (physical pixels).
pub fn show(app: &AppHandle, monitor: MonitorRect) -> Result<()> {
    // Recreate each gesture so the canvas starts blank and no stale loop shows.
    close(app);

    let window = WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=capture".into()),
    )
    .title("Murmur capture")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .focused(true)
    .visible(false)
    .build()
    .context("create the capture overlay")?;

    // Physical position and size: the window must line up pixel-for-pixel with
    // the screenshot, whose crop is computed in the same space.
    window
        .set_position(PhysicalPosition::new(monitor.x, monitor.y))
        .context("position the capture overlay")?;
    window
        .set_size(PhysicalSize::new(monitor.width, monitor.height))
        .context("size the capture overlay")?;
    window
        .set_ignore_cursor_events(false)
        .context("make the capture overlay interactive")?;

    window.show().context("show the capture overlay")?;
    window.set_focus().context("focus the capture overlay")?;
    Ok(())
}

pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        if let Err(err) = window.close() {
            eprintln!("[capture] could not close the overlay: {err}");
        }
    }
}
