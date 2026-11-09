import { invoke } from "@tauri-apps/api/core";

export type TaskStatus =
  | "pending"
  | "paused"
  | "running"
  | "done"
  | "cancelled"
  | "missed"
  | "failed";

export type ActionKind = "open_app" | "open_url" | "run_search" | "remind" | "type_text";

export type ScheduleKind = "one_time" | "recurring";

export interface ScheduledTask {
  id: number;
  prompt: string;
  action: ActionKind;
  payload: string;
  schedule: ScheduleKind;
  due_at_ms: number;
  recurrence: string | null;
  recurrence_start_ms: number | null;
  status: TaskStatus;
  created_at_ms: number;
  updated_at_ms: number;
  fired_at_ms: number | null;
  fire_count: number;
  last_error: string | null;
}

export interface TaskUpdate {
  id: number;
  prompt: string;
  due_at_ms: number;
  action: ActionKind;
  payload: string;
  recurrence: string | null;
}

export type NotchStateName = "idle" | "listening" | "thinking" | "answer" | "error";

/** Payload of `notch://state`, emitted by `notch.rs`. */
export interface NotchPayload {
  state: NotchStateName;
  text: string | null;
}

export type GestureName = "dictation" | "circle_to_ask" | "schedule_task";

/** Bounding box of the freehand loop, normalised to 0..1 of the monitor. */
export interface CircleSelection {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface HotkeyBinding {
  gesture: GestureName;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
}

export interface HotkeySnapshot {
  gesture: GestureName | null;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
}

/** Payload of `audio://level`, emitted every 50 ms while recording. */
export interface AudioLevel {
  level: number;
  recording: boolean;
  device: string | null;
  error: string | null;
}

export type VoiceStyle = "crisp" | "natural" | "formal" | "casual" | "custom";

export interface VoiceSettings {
  style: VoiceStyle;
  polish: boolean;
  custom: string;
}

export interface HotkeySlot {
  gesture: GestureName;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
  enabled: boolean;
}

export interface HotkeySettings {
  hold_to_talk: boolean;
  wake_on_click: boolean;
  wake_on_scroll: boolean;
  bindings: HotkeySlot[];
}

export type PersonaPreset = "assistant" | "expert" | "coach" | "friend" | "custom";

export interface PersonaSettings {
  preset: PersonaPreset;
  name: string;
  instructions: string;
}

export interface AppSettings {
  pinned: string[];
}

export interface InstalledApp {
  name: string;
  target: string;
  /** PNG data URL of the shell icon, when Windows resolves one. */
  icon: string | null;
}

export type ThemeMode = "system" | "dark" | "light";

export interface ThemeSettings {
  mode: ThemeMode;
  accent: string;
}

/** The five-slot Loadout, persisted as one JSON document in SQLite. */
export interface Loadout {
  version: number;
  voice: VoiceSettings;
  hotkeys: HotkeySettings;
  persona: PersonaSettings;
  apps: AppSettings;
  theme: ThemeSettings;
}

export interface ToggleState {
  supported: boolean;
  enabled: boolean;
}

export interface MediaSnapshot {
  title: string;
  artist: string;
  album: string;
  playing: boolean;
  position_seconds: number | null;
  duration_seconds: number | null;
  artwork: string | null;
}

/** System state for the Control Centre. */
export interface ControlSnapshot {
  media: MediaSnapshot | null;
  volume: number;
  muted: boolean;
  volume_supported: boolean;
  wifi: ToggleState;
  bluetooth: ToggleState;
  dark_mode: boolean;
}

export type PowerAction = "lock" | "sleep" | "sign_out" | "restart" | "shutdown";

export interface MicInfo {
  available: boolean;
  device: string | null;
  sample_rate: number;
  channels: number;
  format: string;
  error: string | null;
}

/** True only inside the Tauri webview; the UI stays usable in a plain browser. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api = {
  apiKeyStatus: () => invoke<boolean>("api_key_status"),
  saveApiKey: (key: string) => invoke<void>("save_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),

  listTasks: () => invoke<ScheduledTask[]>("list_tasks"),
  updateTask: (task: TaskUpdate) =>
    invoke<ScheduledTask>("update_task", {
      id: task.id,
      prompt: task.prompt,
      dueAtMs: task.due_at_ms,
      action: task.action,
      payload: task.payload,
      recurrence: task.recurrence,
    }),
  pauseTask: (id: number) => invoke<boolean>("pause_task", { id }),
  resumeTask: (id: number) => invoke<ScheduledTask>("resume_task", { id }),
  cancelTask: (id: number) => invoke<boolean>("cancel_task", { id }),
  deleteTask: (id: number) => invoke<boolean>("delete_task", { id }),

  notchDemo: (state: NotchStateName) => invoke<void>("notch_demo", { state }),
  /** Dismiss the card and return to the resting mark. */
  dismissNotch: () => invoke<void>("notch_hide"),
  /** Copy button: uses the Rust clipboard, which the webview cannot reach. */
  copyText: (text: string) => invoke<void>("notch_copy", { text }),

  /** Hand the finished loop to Rust; it crops, then asks Gemini. */
  submitCircle: (selection: CircleSelection) =>
    invoke<void>("submit_circle", { selection }),
  cancelCircle: () => invoke<void>("cancel_circle"),

  startDictation: () => invoke<void>("start_dictation"),
  stopDictation: () => invoke<string>("stop_dictation"),

  hotkeyBindings: () => invoke<HotkeyBinding[]>("hotkey_bindings"),
  micStatus: () => invoke<MicInfo>("mic_status"),

  getLoadout: () => invoke<Loadout>("get_loadout"),
  saveLoadout: (loadout: Loadout) => invoke<Loadout>("save_loadout", { loadout }),
  patchLoadout: (patch: Partial<Loadout>) => invoke<Loadout>("patch_loadout", { patch }),
  exportLoadout: () => invoke<string>("export_loadout"),
  importLoadout: (json: string) => invoke<Loadout>("import_loadout", { json }),

  controlSnapshot: () => invoke<ControlSnapshot>("control_snapshot"),
  openControlCentre: () => invoke<void>("open_control_centre"),
  openNotchPeek: () => invoke<void>("open_notch_peek"),
  closeNotchPeek: () => invoke<void>("close_notch_peek"),
  openLauncher: () => invoke<void>("open_launcher"),
  closeControlCentre: () => invoke<void>("close_control_centre"),
  mediaToggle: () => invoke<void>("media_toggle"),
  mediaNext: () => invoke<void>("media_next"),
  mediaPrevious: () => invoke<void>("media_previous"),
  setVolume: (level: number) => invoke<void>("set_volume", { level }),
  setMuted: (muted: boolean) => invoke<void>("set_muted", { muted }),
  setRadioEnabled: (kind: "wifi" | "bluetooth", enabled: boolean) =>
    invoke<void>("set_radio_enabled", { kind, enabled }),
  setDarkMode: (enabled: boolean) => invoke<void>("set_dark_mode", { enabled }),
  setWallpaper: (preset: "terraces" | "midnight" | "forest" | "dusk") =>
    invoke<void>("set_wallpaper", { preset }),
  launchPinnedApp: (name: string) => invoke<void>("launch_pinned_app", { name }),
  listInstalledApps: () => invoke<InstalledApp[]>("list_installed_apps"),
  launchInstalledApp: (target: string) => invoke<void>("launch_installed_app", { target }),
  powerAction: (action: PowerAction) => invoke<void>("power_action", { action }),
  showMainWindow: () => invoke<void>("show_main_window"),
};
