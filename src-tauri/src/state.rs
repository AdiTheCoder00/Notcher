use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audio::{AudioClip, Meter, Recorder};
use crate::db::Database;
use crate::gemini;
use crate::hotkeys::Gesture;
use crate::screen;

/// One in-flight circle-to-ask gesture. The screenshot is taken before the
/// overlay is shown, so it can never contain the overlay or the crosshair.
pub struct CircleSession {
    /// Identifies the session for the no-selection timeout.
    pub started: Instant,
    pub monitor: screen::MonitorRect,
    pub screenshot: screen::Capture,
    /// Set by the overlay when the loop is released, in 0..1 monitor space.
    pub selection: Option<screen::NormalizedRect>,
    /// Set when the microphone stops, whichever of key-release or loop-release
    /// happens first.
    pub clip: Option<AudioClip>,
}

/// Recent circle-to-ask turns, so a follow-up inside the window keeps context.
pub struct CircleConversation {
    pub turns: Vec<gemini::Content>,
    pub updated_at: Instant,
}

/// Process-wide state managed by Tauri.
pub struct AppState {
    pub db: Database,
    /// Kept through selection and processing; the lock serializes every finish path.
    pub gesture: tokio::sync::Mutex<Option<Gesture>>,
    /// One recorder: the gestures are mutually exclusive by design.
    pub recorder: Mutex<Recorder>,
    pub dictation_target: Mutex<Option<isize>>,
    /// Peak meter shared with the audio callback and the UI ticker.
    pub meter: Arc<Meter>,
    /// The circle gesture is a two-step capture (keys + loop), so its parts are
    /// collected here until both halves exist.
    pub circle: Mutex<Option<CircleSession>>,
    pub conversation: Mutex<Option<CircleConversation>>,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        let meter = Arc::new(Meter::default());
        Self {
            db,
            gesture: tokio::sync::Mutex::new(None),
            recorder: Mutex::new(Recorder::new(Arc::clone(&meter))),
            dictation_target: Mutex::new(None),
            meter,
            circle: Mutex::new(None),
            conversation: Mutex::new(None),
        }
    }
}
