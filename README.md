# Murmur

A Windows desktop voice assistant. Hold a chord, speak, release — no wake word, no
always-listening microphone, no landing page.

| Chord          | Gesture         | What happens                                                       |
| -------------- | --------------- | ------------------------------------------------------------------ |
| `Ctrl + Win`   | Dictation       | Speech is transcribed, cleaned up and pasted into the focused app.  |
| `Ctrl + Shift` | Circle to ask   | A screen region is captured and sent with your spoken question.     |
| `Ctrl + Alt`   | Schedule        | A spoken task is parsed into a prompt + time and stored in SQLite.  |

Model: `gemini-3.5-flash` (multimodal). The user supplies their own Gemini API key,
stored in the Windows Credential Manager.

## Status

Working end to end: low-level hotkey hook with hold edges, microphone capture →
Gemini transcription → clipboard paste, live input metering, API key storage,
circle-to-ask with a freehand screen selection, voice scheduling through Gemini
function calling, the SQLite task store, a tokio scheduler with restart
catch-up, all five task actions, the task list screen, the notch Control Centre
(media, volume, radios, pinned apps, recent tasks) and the
five-slot Loadout whose voice and persona slots rewrite the Gemini prompts.

## Prerequisites

- **Rust 1.77.2+** with the MSVC toolchain — <https://rustup.rs>. Installed here:
  `1.99.1` (`stable-x86_64-pc-windows-msvc`), with `%USERPROFILE%\.cargo\bin` on
  the user `PATH` so `npm run tauri …` finds `cargo`.
- **Node 20+** (present: v24) and npm.
- **WebView2 runtime** (present) and the MSVC build tools (present).

## Run

```bash
npm install
npm run tauri dev
```

Frontend only, in a browser (no IPC; the notch cycles demo states):

```bash
npm run dev
```

Build a bundle:

```bash
npm run tauri build
```

## Dictation

Hold `Ctrl+Win`, speak, release:

1. **Listening** — the default microphone opens on press and `audio://level`
   drives the notch waveform.
2. **Thinking** — on release the clip becomes a 16 kHz mono i16 WAV in memory and
   is posted to `gemini-3.5-flash` as inline `audio/wav` base64 with the
   transcription instruction.
3. The model is told to fix punctuation and capitalisation, fix obvious grammar
   slips, drop filler words and false starts, keep the speaker's language, and
   output only the final text.
4. **Done** — the text is pasted into the focused app, then the notch hides.

Recording stops automatically at **60 seconds**. If the limit is reached, Murmur reports
an error instead of sending a partial clip.
Device errors also reject the partial recording instead of submitting incomplete speech.

Insertion (`actions.rs`): save the current clipboard text, set the transcript,
simulate Ctrl+V with enigo, then restore the previous text only if the clipboard
still contains the transcript. If the clipboard cannot be read (for example, it
contains an image), Murmur types directly instead of replacing its contents.

### Gemini errors

`gemini.rs` returns typed failures with user-facing messages, so the notch says
what actually happened instead of a status code:

| Condition                  | Message                                                        |
| -------------------------- | -------------------------------------------------------------- |
| No key stored              | "No Gemini API key yet — add one in the Murmur window."          |
| 400/401/403, bad key       | "Gemini rejected the API key — replace it in the Murmur window." |
| 429                        | "Gemini rate limit reached — wait a moment and try again."       |
| Connection/DNS failure     | "No network connection — check your internet and try again."     |
| Request timeout            | "Gemini took too long to answer — try again."                    |

Screen questions and scheduling retry a timeout **once** (30 s per attempt).
Dictation sends once so a retry cannot delay the next recording. Connection
failures and rejected keys are not retried. The API key is read from the Windows Credential Manager on each
request, never from a file.

## Circle to ask

Hold `Ctrl+Shift`, speak, and draw a loop around the part of the screen you mean.

1. The monitor under the cursor is captured **first**, so the overlay can never
   appear in the frame (`screen.rs`, GDI `BitBlt` over the DPI-aware desktop).
2. A transparent, always-on-top window covers exactly that monitor with a
   crosshair (`overlay.rs`); the notch is dismissed.
3. The loop is drawn freehand. Only its bounding box matters; it is sent
   normalised (0..1 of the monitor), mapped to physical pixels with
   `round(normalised × monitor size)`, padded by **16 px**, clamped, and cropped.
4. The crop (native resolution), the full screenshot downscaled to 1280 px wide,
   and the audio go to Gemini in one request, with an instruction to answer the
   spoken question about the circled region concisely.
5. The answer opens in the notch card, where it can be copied; the card dismisses
   itself after 20 s.

Recording starts on the key press, but the **loop's release ends the question**:
whichever of key-release and loop-release happens last supplies the missing half
and the request fires once. `Esc` cancels, and an overlay with no loop is closed
20 s after the keys are released.
The gesture keeps ownership of the recorder while waiting for the loop and while
processing the answer; new voice gestures are ignored and test dictation is rejected
until it finishes.

A repeat within **30 s** continues the same Gemini conversation (`lib.rs` keeps
up to two previous questions and answers, including their screenshots and
audio), so "and what about these?" works. After 30 s the history is dropped
from memory.

Coordinates: the process is per-monitor DPI aware v2, so `GetCursorPos`,
`GetMonitorInfoW` and `BitBlt` all agree on physical pixels. The overlay's CSS
pixels never enter the crop maths, because the selection is normalised against
the monitor.

## Scheduled tasks

Hold `Ctrl+Alt`, speak a request, release.

1. The clip goes to `gemini-3.5-flash` with a `create_task` **function
   declaration**. The prompt includes the current local date, time and IANA
   timezone, so "tomorrow at nine" resolves against the user's clock.
2. Gemini returns one tool call with a `schedule` (a one-time local ISO
   datetime or an RFC 5545 recurrence rule), an `action`
   (`open_app`, `open_url`, `run_search`, `remind`, `type_text`) and a payload.
3. Rust validates the rule, stores the task in SQLite and the task list updates
   immediately.

The tokio scheduler ticks once a second. Tasks live in SQLite. At startup, an
interrupted one-time task is marked `failed` and is not retried automatically;
an interrupted recurring task resumes at its next occurrence. One-time tasks
that came due in the last
**five minutes** fire immediately; older ones become `missed` rather than
firing a pile of stale actions. Recurring tasks that are further behind jump
straight to their next occurrence.

Only the recurrence fields Murmur documents are accepted. Unsupported parts
such as `COUNT`, `UNTIL` and monthly `BYDAY` are rejected instead of silently
ignored. Recurrence intervals keep their original cadence after catch-up, and
editing a task resets its recurrence anchor to the new next-run time. Murmur
allows only one running instance so startup recovery cannot mistake another
instance's active task for interrupted work. Existing recurring tasks use their
current next-run time as the migrated anchor; earlier cadence drift cannot be
reconstructed.

When a task fires, the notch shows `Running: …` and the scheduler asks
`gemini-3.5-flash` — through a `run_action` function call — for the concrete
payload to run right now: the exact text to type, the reminder body, a sharper
search query. The action **kind** is fixed: a reply that changes it is rejected,
so a reminder can never turn into typing into whatever window happens to have
focus. The call has an 8 s budget, and with no key, no network, a timeout or an
invalid reply the task runs exactly as scheduled. The result (`Opened …`,
`Searched for …`, `Reminded: …`) then replaces the running pill; a failure
marks a one-time task `failed` or records `last_error` on a recurring one.

Actions:

| Action                 | Behaviour                                                                 |
| ---------------------- | ------------------------------------------------------------------------- |
| `open_app`             | Resolves the name through App Paths registry keys (HKCU/HKLM, 32/64-bit) and Start Menu `.lnk` files, then launches through the shell. |
| `open_url`             | Accepts absolute `http`/`https` URLs only.                                |
| `run_search`           | Opens the default browser at a Google search for the payload.             |
| `remind`               | Shows a Windows toast through `tauri-plugin-notification`.                |
| `type_text`            | Pastes into the focused window with the same clipboard-preserving path dictation uses. |

The Scheduled section of the main window lists up to 200 tasks, showing active
tasks before history, with their next run, recurrence, action and error state.
Pending tasks can be paused; paused tasks can be resumed. Tasks can be edited
(prompt, next run, recurrence, action, payload) or deleted when not running.

## Control Centre

Clicking or scrolling the resting mark grows the notch window downward into a
424 × 688 panel (`notch::set_control_open`); the X button or `Esc` closes it,
and any hotkey press closes it first so a dictation result is never hidden.

- **Now playing** comes from SMTC
  (`GlobalSystemMediaTransportControlsSessionManager`): title, artist, album and
  play/pause state, with play/pause, next and previous actions. With no session
  the transport buttons are disabled.
- **Volume** is the master endpoint scalar through `IAudioEndpointVolume`
  (slider plus mute).
- **Quick toggles** are WinRT `Radio` state for Wi-Fi and Bluetooth (shown as
  Unavailable when Windows denies radio access) and the Windows app theme,
  written to the Personalize registry key and broadcast with `WM_SETTINGCHANGE`.
- **Wallpapers** set the Windows desktop background from Murmur's built-in
  image and gradient presets.
- **Pinned apps** are the Loadout's list, resolved through App Paths and the
  Start Menu when launched; **recent tasks** are `list_tasks` sorted by
  `updated_at_ms`.

The panel polls system state every 2.5 s while open and debounces slider writes
by 140 ms. Core Audio, WinRT and DDC/CI calls run on blocking threads with COM
initialised per call, so a slow device never stalls the webview.
The focus timer uses a clock deadline, so it catches up after delayed callbacks or
sleep. Closing the launcher restores the previous app only if the notch still has
focus. Power actions enable the account's shutdown privilege for the call and restore
its previous state afterwards; accounts without that privilege receive an error.

## Loadout

Five slots — **Voice**, **Hotkeys**, **Persona**, **Apps**, **Theme** — are drawn
as a radial map around the Murmur mark. Clicking a slot opens its editor below
the map. Everything lives in one JSON document in the `settings` table under the
key `loadout`, so export and import are a copy of the same representation
(`murmur-loadout.json`, normalized on the way in).

- Edits are debounced by 450 ms, saved through `save_loadout`, normalized in
  Rust (bounded text, known presets, duplicate or modifier-less chords disabled)
  and returned so the UI mirrors what was actually stored.
- Every save is broadcast as `loadout://changed`, which updates the notch's theme
  and its click/scroll wake preferences without a restart.
- **Voice** style, polish and custom text become
  `VoiceSettings::dictation_guidance()`, appended to `TRANSCRIBE_PROMPT` for both
  the hotkey and test-dictation paths.
- **Persona** preset, name and instructions become
  `PersonaSettings::answer_guidance()`, appended to the circle-answer system
  instruction.
- **Hotkeys** rebinds chords live through `hotkeys::set_bindings`, and the
  **Theme** slot applies immediately through `applyTheme`.

Both prompt slots render the exact appended text under the editor, so what the
model receives is visible before a request is made.

## Hotkeys

`hotkeys.rs` installs `WH_KEYBOARD_LL` on its own thread with its own message
loop. `RegisterHotKey` — and therefore `tauri-plugin-global-shortcut` — needs a
real virtual key, so modifier-only chords like `Ctrl+Win` cannot be registered,
and hold-to-talk needs the release edge anyway.

- Bindings are data (`HotkeyConfig`), resolved by **exact** modifier match, so
  rolling from `Ctrl+Shift` into `Ctrl+Win` produces a clean release/press pair
  instead of firing two gestures.
- Modifier state is tracked from hook events, not `GetAsyncKeyState`: inside a
  low-level hook the OS key state can still be stale for the key that triggered
  the callback.
- Injected events (`LLKHF_INJECTED`) are ignored, so enigo's own typing and the
  Ctrl+V paste cannot re-enter the state machine.
- The **Win key-up is swallowed** (the hook returns `1` instead of chaining) so
  Windows does not open the Start menu after `Ctrl+Win`. It is only swallowed if
  a Win binding fired and no other key was pressed during that Win hold, so
  `Win+E` / `Ctrl+Win+→` keep working. Win held **without** Ctrl is untouched and
  still opens the Start menu normally.
- **Alt on its own is not a usable chord.** Normalization keeps the value but
  disables it, because holding Alt is how `Alt+Tab`, `Alt+F4` and `Alt+Space`
  start — a bare-Alt gesture would fire while the user switches windows. Add
  Ctrl, Shift or Win to the chord instead.
- `hotkeys::stop()` posts `WM_QUIT` to the hook thread, which unhooks and exits.
- The settings list and `hotkey_bindings` command read the same config, so
  rebinding later only changes data.

## Audio

- On hold-start the default input device is opened on a dedicated thread (the
  cpal stream is `!Send` on some backends); a native 16 kHz mono f32 stream is
  requested when supported, otherwise the device default is downmixed and
  resampled in memory.
- On hold-end the clip is returned as 16 kHz mono i16 and wrapped in a WAV
  container in memory.
- A peak meter is updated from the audio callback and `audio://level` is emitted
  every 50 ms while recording, with falloff so the waveform does not flicker.
- A missing or busy microphone is reported, not fatal: the error appears in the
  notch and the debug panel, and the app keeps running.

## Notch

A frameless, transparent, always-on-top window pinned to the top centre of the
primary monitor, never focused — a focused overlay would eat the keystrokes
dictation is meant to paste. It is click-through in every state except `answer`,
where the copy button needs the pointer (`notch.rs` flips
`set_ignore_cursor_events` per state; CSS `pointer-events` cannot do this).

| State       | Pill                                                                     |
| ----------- | ------------------------------------------------------------------------ |
| `idle`      | 40 px mark, click-through                                                 |
| `listening` | expands, live waveform fed by `audio://level` at 20 Hz                     |
| `thinking`  | three animated dots                                                       |
| `answer`    | card with markdown (GFM), copy button, 20 s auto-dismiss paused on hover   |
| `error`     | short red message                                                         |

Width, height and corner radius spring between states via framer-motion
`layout`; reduced-motion users get instant transitions. Colours come from CSS
tokens that follow `prefers-color-scheme`, which WebView2 maps to the Windows
light/dark app theme. The answer card's copy button and dismissal go through
Rust (`notch_copy`, `notch_hide`), not browser APIs.

### Event bus

`src/lib/bus.ts` is the typed bridge: `EVENTS` plus an `EventPayloads` map mirror
the Rust names (`notch://state`, `audio://level`, `hotkeys://state`,
`task://due`). `useRustEvent(name, handler)` subscribes on mount, always calls
the latest handler, and is a no-op in a plain browser.

## Debug panel

The settings window shows live state straight from the hook and the meter:
modifiers held, active combo, input level, recording state, device name, mic
errors, and the active binding table.

## Layout

```
src/                      React + TypeScript + Tailwind
  windows/MainWindow.tsx  settings: API key, Loadout, hotkeys, tasks, hook debug, notch preview
  windows/NotchWindow.tsx notch view and Control Centre (index.html?window=notch)
  windows/CaptureOverlay.tsx circle selection overlay (index.html?window=capture)
  components/notch/       pill states, answer card, waveform, springs
  components/control/     Control Centre panel (media, sliders, toggles, apps, tasks)
  components/loadout/     radial Loadout map and slot editors
  lib/ipc.ts              typed invoke() wrappers and event payloads
  lib/bus.ts              typed Rust → frontend event names and payloads
src-tauri/src/
  lib.rs                  setup, command surface, gesture → recording → Gemini → paste
  hotkeys.rs              WH_KEYBOARD_LL hook, config-driven chords, Win-release swallow
  audio.rs                cpal capture, 16 kHz mono i16 + WAV, peak meter emitter
  gemini.rs               Credential Manager key, typed errors, generateContent client
  actions.rs              clipboard paste, app/URL/search launch, toast, typing
  screen.rs               GDI monitor capture, physical-pixel crop, downscale, PNG
  overlay.rs              transparent crosshair overlay window per gesture
  control.rs              SMTC media, Core Audio volume, radios, theme
  loadout.rs              five-slot Loadout model, normalization, prompt guidance
  scheduler.rs            task model, RRULE subset, catch-up + tokio scheduler
  db.rs                   SQLite: scheduled_tasks, settings/Loadout, migrations
  notch.rs                notch window: create, position, resize, click-through, events
  state.rs                managed state: db + recorder + meter + circle session
```

## Privacy invariants

- Microphone audio exists only in a `Vec<f32>`/`Vec<i16>` in RAM and is dropped
  after the request. It is never written to a file or the clipboard.
- Screenshots are encoded in memory and posted straight to Gemini.
- Dictated text passes through the clipboard only long enough to paste, and the
  previous clipboard text is restored afterwards.
- SQLite stores text only: task prompt, schedule, action, payload, status and
  timestamps.
- The API key lives in the Windows Credential Manager (service `murmur`,
  account `gemini-api-key`), not in a config file or `.env`.

## Conventions

- No `unwrap()` in production paths — errors are propagated with `anyhow` and
  mapped to strings at the command boundary.
- The notch window is created with `focus: false` and never calls `set_focus`.


## Regression checks

```bash
cargo test --manifest-path src-tauri/Cargo.toml --offline
npm run check:notch-state
npm run check:notch
python -m unittest discover -s .github/prompts/design/scripts/tests -v
```

The state check uses headless Edge with synthetic Rust events and a disposable
profile. It checks that clipboard history and the focus timer survive closing
and reopening the panel. It does not require the desktop automation helper.
The two upstream catalog-maintenance test modules skip when their helper scripts
are not bundled; the shipped search and data tests still run.

The logo, icon, and CIP generators require `google-genai` (and `pillow` for image
inputs). They use Gemini's `v1beta` API for preview models and return a nonzero
exit status when a requested output fails. Successful live generation also
requires model quota on the supplied API key.
