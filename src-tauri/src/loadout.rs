//! The five-slot Loadout: preferences that shape the assistant's voice,
//! behaviour, appearance and quick-launch surface.
//!
//! Everything is one JSON document in the `settings` table, which makes export
//! and import a straight copy of the same representation. The hook and the
//! Gemini prompts read this document; the UI never owns state the backend does
//! not persist.

use serde::{Deserialize, Serialize};

use crate::hotkeys::{Binding, Gesture};

pub const SETTINGS_KEY: &str = "loadout";

/// Emitted after every save/import so the notch window picks up a new theme,
/// wake preference or pinned app list without an app restart.
pub const CHANGED_EVENT: &str = "loadout://changed";

pub const DEFAULT_ACCENT: &str = "#d0ad5e";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub voice: VoiceSettings,
    #[serde(default)]
    pub hotkeys: HotkeySettings,
    #[serde(default)]
    pub persona: PersonaSettings,
    #[serde(default)]
    pub apps: AppSettings,
    #[serde(default)]
    pub theme: ThemeSettings,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            version: 1,
            voice: VoiceSettings::default(),
            hotkeys: HotkeySettings::default(),
            persona: PersonaSettings::default(),
            apps: AppSettings::default(),
            theme: ThemeSettings::default(),
        }
    }
}

impl Loadout {
    /// Keeps imported JSON safe: lengths are bounded, unknown presets fall back
    /// to defaults, and duplicate or modifier-less chords are disabled so the
    /// low-level hook can never match every key press.
    pub fn normalize(mut self) -> Self {
        self.version = 1;

        self.voice.style = normalize_choice(
            &self.voice.style,
            &["crisp", "natural", "formal", "casual", "custom"],
            "natural",
        );
        self.voice.custom = clean_text(&self.voice.custom, 600);

        self.persona.preset = normalize_choice(
            &self.persona.preset,
            &["assistant", "expert", "coach", "friend", "custom"],
            "assistant",
        );
        self.persona.name = clean_text(&self.persona.name, 60);
        if self.persona.name.is_empty() {
            self.persona.name = PersonaSettings::default().name;
        }
        self.persona.instructions = clean_text(&self.persona.instructions, 1500);

        let defaults = HotkeySettings::default();
        let mut seen = Vec::new();
        let mut bindings = Vec::with_capacity(Gesture::ALL.len());
        for gesture in Gesture::ALL {
            let mut slot = self
                .hotkeys
                .bindings
                .iter()
                .find(|slot| slot.gesture == gesture)
                .cloned()
                .or_else(|| {
                    defaults
                        .bindings
                        .iter()
                        .find(|slot| slot.gesture == gesture)
                        .cloned()
                })
                .unwrap_or_default();

            if !slot.ctrl && !slot.shift && !slot.alt && !slot.win {
                slot.enabled = false;
            }
            // Alt on its own is the prefix of Alt+Tab, Alt+F4 and Alt+Space: held as a
            // push-to-talk chord it fires a gesture every time the user switches
            // windows. Keep the chord stored (so the editor can show what was picked)
            // but never hand it to the hook.
            if slot.alt && !slot.ctrl && !slot.shift && !slot.win {
                slot.enabled = false;
            }
            let chord = (slot.ctrl, slot.shift, slot.alt, slot.win);
            if slot.enabled && seen.contains(&chord) {
                slot.enabled = false;
            } else if slot.enabled {
                seen.push(chord);
            }
            bindings.push(slot);
        }
        self.hotkeys.bindings = bindings;

        self.apps.pinned = self
            .apps
            .pinned
            .iter()
            .map(|app| clean_text(app, 80))
            .filter(|app| !app.is_empty())
            .fold(Vec::new(), |mut apps, app| {
                if apps.len() < 12
                    && !apps
                        .iter()
                        .any(|seen: &String| seen.eq_ignore_ascii_case(&app))
                {
                    apps.push(app);
                }
                apps
            });

        self.theme.mode =
            normalize_choice(&self.theme.mode, &["system", "dark", "light"], "system");
        if !is_hex_color(&self.theme.accent) {
            self.theme.accent = DEFAULT_ACCENT.to_string();
        }

        self
    }

    /// The bindings the keyboard hook should resolve, in gesture order.
    pub fn active_bindings(&self) -> Vec<Binding> {
        self.hotkeys
            .bindings
            .iter()
            .filter(|slot| slot.enabled)
            .map(|slot| Binding {
                gesture: slot.gesture,
                ctrl: slot.ctrl,
                shift: slot.shift,
                alt: slot.alt,
                win: slot.win,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceSettings {
    /// crisp | natural | formal | casual | custom
    pub style: String,
    /// When false, dictation transcribes verbatim instead of cleaning speech.
    pub polish: bool,
    /// Extra guidance appended to the dictation prompt.
    pub custom: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            style: "natural".to_string(),
            polish: true,
            custom: String::new(),
        }
    }
}

impl VoiceSettings {
    /// Extra prompt text for dictation. Appended to the base transcribe prompt
    /// in Gemini, so changing this genuinely changes what the model does.
    pub fn dictation_guidance(&self) -> String {
        let style = match self.style.as_str() {
            "crisp" => "Write tight, decisive sentences and prefer the shortest wording that keeps the meaning.",
            "formal" => "Use polished, professional language and complete sentences.",
            "casual" => "Keep the result relaxed and conversational; contractions are welcome.",
            "custom" => "Follow the speaker's custom voice instructions below precisely.",
            _ => "Keep the speaker's natural rhythm, contractions and tone.",
        };

        let polish = if self.polish {
            "Fix punctuation, capitalisation, obvious grammar slips and filler words."
        } else {
            "Stay verbatim: keep filler words, false starts and the speaker's exact wording. Do not polish."
        };

        let mut guidance = format!("Voice style: {style} {polish}");
        let custom = self.custom.trim();
        if !custom.is_empty() {
            guidance.push_str("\nCustom dictation instructions: ");
            guidance.push_str(custom);
        }
        guidance
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonaSettings {
    /// assistant | expert | coach | friend | custom
    pub preset: String,
    pub name: String,
    /// Extra guidance appended to every answer prompt.
    pub instructions: String,
}

impl Default for PersonaSettings {
    fn default() -> Self {
        Self {
            preset: "assistant".to_string(),
            name: "Murmur".to_string(),
            instructions: String::new(),
        }
    }
}

impl PersonaSettings {
    /// Extra prompt text for answers. Appended to the circle-answer prompt.
    pub fn answer_guidance(&self) -> String {
        let preset = match self.preset.as_str() {
            "expert" => "Speak like a senior domain expert: precise, confident and compact.",
            "coach" => "Be an encouraging coach: warm, direct and action-oriented.",
            "friend" => "Be a friendly, casual companion; use plain language and light warmth.",
            "custom" => "Follow the persona instructions below precisely.",
            _ => "Be a capable, neutral desktop assistant.",
        };

        let mut guidance = format!("Persona: {}. {preset}", self.name.trim());
        let custom = self.instructions.trim();
        if !custom.is_empty() {
            guidance.push_str("\nPersona instructions: ");
            guidance.push_str(custom);
        }
        guidance
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeySettings {
    /// True for hold-to-talk, false for tap-to-start / tap-to-stop.
    pub hold_to_talk: bool,
    /// Whether clicking the resting notch wakes the Control Centre.
    pub wake_on_click: bool,
    /// Whether scrolling over the resting notch wakes the Control Centre.
    pub wake_on_scroll: bool,
    pub bindings: Vec<HotkeySlot>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            hold_to_talk: true,
            wake_on_click: true,
            wake_on_scroll: true,
            bindings: vec![
                HotkeySlot {
                    gesture: Gesture::Dictation,
                    ctrl: true,
                    shift: false,
                    alt: false,
                    win: true,
                    enabled: true,
                },
                HotkeySlot {
                    gesture: Gesture::CircleToAsk,
                    ctrl: true,
                    shift: true,
                    alt: false,
                    win: false,
                    enabled: true,
                },
                HotkeySlot {
                    gesture: Gesture::ScheduleTask,
                    ctrl: true,
                    shift: false,
                    alt: true,
                    win: false,
                    enabled: true,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeySlot {
    pub gesture: Gesture,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    pub enabled: bool,
}

impl Default for HotkeySlot {
    fn default() -> Self {
        Self {
            gesture: Gesture::Dictation,
            ctrl: true,
            shift: false,
            alt: false,
            win: false,
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// App names resolved through App Paths and the Start Menu at launch time.
    pub pinned: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pinned: vec![
                "Spotify".to_string(),
                "Notepad".to_string(),
                "Settings".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeSettings {
    /// system | dark | light
    pub mode: String,
    /// #rrggbb accent used by both windows.
    pub accent: String,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: "system".to_string(),
            accent: DEFAULT_ACCENT.to_string(),
        }
    }
}

fn default_version() -> u32 {
    1
}

fn normalize_choice(value: &str, allowed: &[&str], fallback: &str) -> String {
    let trimmed = value.trim().to_ascii_lowercase();
    if allowed.contains(&trimmed.as_str()) {
        trimmed
    } else {
        fallback.to_string()
    }
}

fn clean_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_loadout_round_trips_through_json() {
        let loadout = Loadout::default();
        let json = serde_json::to_string(&loadout).expect("serialize");
        let parsed: Loadout = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(loadout, parsed.normalize());
    }

    #[test]
    fn voice_style_changes_the_dictation_guidance() {
        let natural = VoiceSettings::default();
        let crisp = VoiceSettings {
            style: "crisp".into(),
            ..VoiceSettings::default()
        };
        assert_ne!(natural.dictation_guidance(), crisp.dictation_guidance());
        assert!(crisp.dictation_guidance().contains("shortest wording"));

        let verbatim = VoiceSettings {
            polish: false,
            ..VoiceSettings::default()
        };
        assert!(verbatim.dictation_guidance().contains("verbatim"));
    }

    #[test]
    fn persona_changes_the_answer_guidance() {
        let assistant = PersonaSettings::default();
        let expert = PersonaSettings {
            preset: "expert".into(),
            name: "Ada".into(),
            instructions: "Lead with the answer.".into(),
        };
        assert_ne!(assistant.answer_guidance(), expert.answer_guidance());
        assert!(expert.answer_guidance().contains("Ada"));
        assert!(expert.answer_guidance().contains("Lead with the answer."));
    }

    #[test]
    fn normalization_drops_duplicate_and_empty_chords() {
        let mut loadout = Loadout::default();
        loadout.hotkeys.bindings.push(HotkeySlot {
            gesture: Gesture::ScheduleTask,
            ctrl: true,
            shift: false,
            alt: false,
            win: true,
            enabled: true,
        });
        loadout.hotkeys.bindings.push(HotkeySlot {
            gesture: Gesture::Dictation,
            ctrl: false,
            shift: false,
            alt: false,
            win: false,
            enabled: true,
        });

        let normalized = loadout.normalize();
        assert_eq!(normalized.hotkeys.bindings.len(), 3);
        let active = normalized.active_bindings();
        assert!(!active.is_empty());
        for binding in &active {
            assert!(binding.ctrl || binding.shift || binding.alt || binding.win);
            let same = active
                .iter()
                .filter(|other| {
                    (other.ctrl, other.shift, other.alt, other.win)
                        == (binding.ctrl, binding.shift, binding.alt, binding.win)
                })
                .count();
            assert_eq!(same, 1);
        }
    }

    #[test]
    fn normalization_disables_alt_only_chords() {
        let mut loadout = Loadout::default();
        for slot in &mut loadout.hotkeys.bindings {
            if slot.gesture == Gesture::CircleToAsk {
                slot.ctrl = false;
                slot.shift = false;
                slot.alt = true;
                slot.win = false;
                slot.enabled = true;
            }
        }

        let normalized = loadout.normalize();
        assert!(
            normalized.active_bindings().iter().all(|binding|
                !(binding.alt && !binding.ctrl && !binding.shift && !binding.win)
            ),
            "a bare Alt chord must never reach the keyboard hook"
        );
        let circle = normalized
            .hotkeys
            .bindings
            .iter()
            .find(|slot| slot.gesture == Gesture::CircleToAsk)
            .expect("circle slot");
        assert!(circle.alt && !circle.enabled);
    }

    #[test]
    fn normalization_bounds_imported_values() {
        let noisy = Loadout {
            voice: VoiceSettings {
                style: "shouty".into(),
                custom: "x".repeat(2_000),
                ..VoiceSettings::default()
            },
            theme: ThemeSettings {
                mode: "neon".into(),
                accent: "red".into(),
            },
            apps: AppSettings {
                pinned: (0..40).map(|index| format!("App {index}")).collect(),
            },
            ..Loadout::default()
        }
        .normalize();

        assert_eq!(noisy.voice.style, "natural");
        assert_eq!(noisy.voice.custom.chars().count(), 600);
        assert_eq!(noisy.theme.mode, "system");
        assert_eq!(noisy.theme.accent, DEFAULT_ACCENT);
        assert_eq!(noisy.apps.pinned.len(), 12);
    }
}
