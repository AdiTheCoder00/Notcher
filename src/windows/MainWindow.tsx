import { useCallback, useEffect, useRef, useState } from "react";
import { EVENTS, useRustEvent } from "../lib/bus";
import {
  api,
  isTauri,
  type GestureName,
  type HotkeyBinding,
  type Loadout,
  type NotchStateName,
  type ScheduledTask,
  type TaskUpdate,
} from "../lib/ipc";
import { ApiKeyCard } from "../components/ApiKeyCard";
import { UpdateCard } from "../components/UpdateCard";
import { TaskList } from "../components/TaskList";
import { DebugPanel } from "../components/DebugPanel";
import { KeyCombo } from "../components/Keycap";
import { LoadoutSection } from "../components/loadout/Loadout";
import { ControlCentre } from "../components/control/ControlCentre";
import { applyTheme, defaultLoadout } from "../lib/theme";
import {
  AppsIcon,
  CheckIcon,
  ChevronRightIcon,
  DownloadIcon,
  KeyboardIcon,
  MicIcon,
  MurmurMark,
  PaletteIcon,
} from "../components/Icons";

const GESTURE_COPY: Record<GestureName, { title: string; detail: string }> = {
  dictation: {
    title: "Dictation",
    detail: "Speak, then find the transcript in the app you were already using.",
  },
  circle_to_ask: {
    title: "Circle to ask",
    detail: "Hold and ask about what is on screen. The question goes out with a screenshot.",
  },
  schedule_task: {
    title: "Schedule by voice",
    detail: "Describe the task; it lands in the list below at the time you said.",
  },
};

const NOTCH_PREVIEWS: { label: string; value: NotchStateName }[] = [
  { label: "Idle mark", value: "idle" },
  { label: "Listening", value: "listening" },
  { label: "Thinking", value: "thinking" },
  { label: "Answer", value: "answer" },
  { label: "Error", value: "error" },
];

const NAV_ITEMS = [
  { id: "overview", label: "Overview", icon: AppsIcon },
  { id: "connection", label: "Connection", icon: MicIcon },
  { id: "loadout", label: "Loadout", icon: PaletteIcon },
  { id: "hotkeys", label: "Shortcuts", icon: KeyboardIcon },
  { id: "scheduled", label: "Scheduled", icon: CheckIcon },
  { id: "diagnostics", label: "Diagnostics", icon: ChevronRightIcon },
  { id: "updates", label: "Updates", icon: DownloadIcon },
  { id: "notch-preview", label: "Notch preview", icon: MurmurMark },
] as const;

function keysFor(binding: HotkeyBinding): string[] {
  const keys: string[] = [];
  if (binding.ctrl) keys.push("Ctrl");
  if (binding.shift) keys.push("Shift");
  if (binding.alt) keys.push("Alt");
  if (binding.win) keys.push("Win");
  return keys;
}

export default function MainWindow() {
  const [hasKey, setHasKey] = useState(false);
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  const [loadout, setLoadout] = useState<Loadout>(defaultLoadout);
  const [notice, setNotice] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [loadoutReady, setLoadoutReady] = useState(false);
  const [activeSection, setActiveSection] = useState(() => {
    const section = window.location.hash.slice(1);
    return NAV_ITEMS.some((item) => item.id === section) ? section : "overview";
  });

  const savedJson = useRef<string | null>(null);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const loadoutAnchor = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setLoaded(true);
      setLoadoutReady(true);
      return;
    }
    try {
      const [keyStatus, list, saved] = await Promise.allSettled([
        api.apiKeyStatus(),
        api.listTasks(),
        api.getLoadout(),
      ]);
      if (keyStatus.status === "fulfilled") setHasKey(keyStatus.value);
      if (list.status === "fulfilled") setTasks(list.value);
      if (saved.status === "fulfilled") {
        setLoadout(saved.value);
        savedJson.current = JSON.stringify(saved.value);
        setLoadoutReady(true);
      }
      const failures = [keyStatus, list, saved].filter((result) => result.status === "rejected");
      if (failures.length) setNotice(failures.map((result) => String(result.reason)).join("; "));
    } catch (err) {
      setNotice(String(err));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const syncActiveSection = () => {
      const section = window.location.hash.slice(1);
      if (NAV_ITEMS.some((item) => item.id === section)) setActiveSection(section);
    };

    window.addEventListener("hashchange", syncActiveSection);
    return () => window.removeEventListener("hashchange", syncActiveSection);
  }, []);

  /** Task-only refresh, so a due reminder never clobbers an in-flight edit. */
  const refreshTasks = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setTasks(await api.listTasks());
    } catch (err) {
      setNotice(String(err));
    }
  }, []);

  // The theme is a Loadout slot, so it applies to the whole document as soon
  // as it changes, before the debounced SQLite write.
  useEffect(() => {
    applyTheme(loadout.theme);
  }, [loadout.theme]);

  // Every edit is persisted, but a slider-like burst of edits only writes
  // once. The backend normalizes the document and returns what it stored, so
  // duplicates and invalid values are reflected back in the UI.
  useEffect(() => {
    if (!loadoutReady) return;
    const json = JSON.stringify(loadout);
    const timer = window.setTimeout(() => {
      if (!isTauri()) {
        savedJson.current = json;
        return;
      }
      saveQueue.current = saveQueue.current.then(async () => {
        if (savedJson.current === json) return;
        try {
          const previous = savedJson.current ? JSON.parse(savedJson.current) as Loadout : null;
          const patch: Partial<Loadout> = {};
          for (const key of ["voice", "hotkeys", "persona", "apps", "theme"] as const) {
            if (!previous || JSON.stringify(previous[key]) !== JSON.stringify(loadout[key])) {
              Object.assign(patch, { [key]: loadout[key] });
            }
          }
          if (!Object.keys(patch).length) return;
          const saved = await api.patchLoadout(patch);
          savedJson.current = JSON.stringify(saved);
          setLoadout((current) => JSON.stringify(current) === json ? saved : current);
        } catch (err) {
          setNotice(String(err));
        }
      });
    }, 450);

    return () => window.clearTimeout(timer);
  }, [loadout, loadoutReady]);

  useRustEvent(EVENTS.taskDue, () => {
    void refreshTasks();
  });

  useRustEvent(EVENTS.loadoutChanged, (remote) => {
    if (!savedJson.current) return;
    const previous = JSON.parse(savedJson.current) as Loadout;
    savedJson.current = JSON.stringify(remote);
    setLoadout((current) => {
      const merged = { ...current };
      for (const key of ["voice", "hotkeys", "persona", "apps", "theme"] as const) {
        if (JSON.stringify(current[key]) === JSON.stringify(previous[key])) {
          Object.assign(merged, { [key]: remote[key] });
        }
      }
      return merged;
    });
  });

  useRustEvent(EVENTS.loadoutFocus, () => {
    loadoutAnchor.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  });

  const bindings: HotkeyBinding[] = loadout.hotkeys.bindings
    .filter((slot) => slot.enabled)
    .map((slot) => ({
      gesture: slot.gesture,
      ctrl: slot.ctrl,
      shift: slot.shift,
      alt: slot.alt,
      win: slot.win,
    }));
  const dictationBinding = bindings.find((binding) => binding.gesture === "dictation");
  const activeTasks = tasks.filter(
    (task) =>
      task.status === "pending" || task.status === "paused" || task.status === "running",
  ).length;
  const activePage = NAV_ITEMS.find((item) => item.id === activeSection) ?? NAV_ITEMS[0];

  async function pauseTask(id: number) {
    try {
      await api.pauseTask(id);
      await refreshTasks();
    } catch (err) {
      setNotice(String(err));
    }
  }

  async function resumeTask(id: number) {
    try {
      await api.resumeTask(id);
      await refreshTasks();
    } catch (err) {
      setNotice(String(err));
    }
  }

  async function deleteTask(id: number) {
    if (!window.confirm("Delete this scheduled task?")) return;
    try {
      await api.deleteTask(id);
      await refreshTasks();
    } catch (err) {
      setNotice(String(err));
    }
  }

  async function saveTask(task: TaskUpdate) {
    try {
      await api.updateTask(task);
      await refreshTasks();
    } catch (err) {
      setNotice(String(err));
      throw err;
    }
  }

  async function previewNotch(state: NotchStateName) {
    if (!isTauri()) {
      setNotice("The notch only exists inside the app window — run `npm run tauri dev`.");
      return;
    }
    try {
      await api.notchDemo(state);
    } catch (err) {
      setNotice(String(err));
    }
  }

  async function testDictation() {
    if (!isTauri()) {
      setNotice("Recording needs the desktop app.");
      return;
    }
    setTesting(true);
    setNotice("Recording for three seconds…");
    try {
      await api.startDictation();
      await new Promise((resolve) => setTimeout(resolve, 3000));
      const transcript = await api.stopDictation();
      setNotice(transcript.length > 0 ? transcript : "No speech detected.");
    } catch (err) {
      setNotice(String(err));
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="dashboard-shell">
      <aside className="dashboard-sidebar" aria-label="Dashboard sidebar">
        <a className="dashboard-brand" href="#overview" aria-label="Murmur overview">
          <span className="dashboard-brand-mark">
            <MurmurMark size={17} />
          </span>
          <span>
            <strong>Murmur</strong>
            <small>VOICE WORKSPACE</small>
          </span>
        </a>

        <p className="dashboard-nav-label">WORKSPACE</p>
        <nav className="dashboard-nav" aria-label="Dashboard sections">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            return (
              <a
                key={item.id}
                href={`#${item.id}`}
                className="dashboard-nav-link"
                aria-current={activeSection === item.id ? "location" : undefined}
                onClick={() => setActiveSection(item.id)}
              >
                <Icon size={16} />
                <span>{item.label}</span>
              </a>
            );
          })}
        </nav>

        <div className="dashboard-sidebar-bottom">
          <div className="dashboard-sidebar-status">
            <span
              className={`dashboard-status-mark ${hasKey ? "is-ready" : "is-pending"}`}
              aria-hidden="true"
            />
            <span>{hasKey ? "Key saved" : "Setup required"}</span>
          </div>
          <p>Audio stays on your device until you ask Murmur to process it.</p>
        </div>
      </aside>

      <main className="dashboard-main">
        <header className="dashboard-topbar">
          <div className="dashboard-breadcrumb">
            <span>Workspace</span>
            <span aria-hidden="true">/</span>
            <strong>{activePage.label}</strong>
          </div>
          <a
            href="#connection"
            className={`dashboard-connection ${hasKey ? "is-ready" : "is-pending"}`}
            aria-live="polite"
          >
            <span className="dashboard-status-mark" />
            <span>
              {loaded ? (hasKey ? "Key saved" : "Connect an API key") : "Checking saved key"}
            </span>
            <ChevronRightIcon size={14} />
          </a>
        </header>

        <div className="dashboard-content">
          <section id="overview" className="dashboard-overview dashboard-anchor">
            <div className="dashboard-overview-copy">
              <h1>A quieter way to work.</h1>
              <p>
                Hold a shortcut, speak naturally, and keep moving. Murmur handles dictation,
                quick questions, and reminders.
              </p>
              <div className="dashboard-overview-actions">
                <button
                  type="button"
                  onClick={() => void testDictation()}
                  disabled={testing}
                  className="dashboard-primary-button"
                >
                  <MicIcon size={15} />
                  {testing ? "Recording…" : "Test dictation"}
                </button>
                <a className="dashboard-text-link" href="#hotkeys">
                  View shortcuts <ChevronRightIcon size={14} />
                </a>
                <button
                  type="button"
                  className="dashboard-subtle-button"
                  disabled={!isTauri()}
                  onClick={() => void api.openLauncher().catch((err) => setNotice(String(err)))}
                >
                  Open Control Centre
                </button>
              </div>
            </div>

            <div className="dashboard-quick-start">
              <div className="dashboard-quick-start-heading">
                <span className="dashboard-quick-icon">
                  <MicIcon size={16} />
                </span>
                <div>
                  <p>Ready when you are</p>
                  <span>Your dictation shortcut</span>
                </div>
              </div>
              <div className="dashboard-shortcut-line">
                {dictationBinding ? (
                  <KeyCombo keys={keysFor(dictationBinding)} />
                ) : (
                  <span className="dashboard-unassigned">No shortcut assigned</span>
                )}
                <span className="dashboard-shortcut-description">
                  {dictationBinding ? "Hold, speak, release" : "Set one in Loadout"}
                </span>
              </div>
              <div className="dashboard-quick-rule" />
              <div className="dashboard-quick-foot">
                <span>{bindings.length} active shortcuts</span>
                <a
                  href="#hotkeys"
                  aria-label="Configure shortcuts"
                  className="inline-flex h-6 w-6 items-center justify-center rounded-full focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
                >
                  <ChevronRightIcon size={15} />
                </a>
              </div>
            </div>
          </section>

          <section className="dashboard-media-frame" aria-label="Now playing preview">
            <div className="dashboard-media-screen">
              <img
                className="dashboard-media-image"
                src="/images/terraced-fields.jpg"
                alt="Terraced fields across a hillside"
                fetchPriority="high"
              />
              <div className="dashboard-media-shade" aria-hidden="true" />
              <div className="dashboard-media-widget">
                <ControlCentre onClose={() => undefined} compact />
              </div>
            </div>
          </section>

          <div className="dashboard-metrics" aria-label="Workspace summary">
            <div className="dashboard-metric">
              <span>API key</span>
              <strong className={hasKey ? "metric-ready" : "metric-pending"}>
                {loaded ? (hasKey ? "Saved" : "Needs setup") : "Checking"}
              </strong>
            </div>
            <div className="dashboard-metric">
              <span>Shortcuts</span>
              <strong>{bindings.length} active</strong>
            </div>
            <div className="dashboard-metric">
              <span>Scheduled</span>
              <strong>{loaded ? `${activeTasks} tasks` : "Loading"}</strong>
            </div>
          </div>

          {notice ? (
            <div className="dashboard-notice" role="status">
              <p>{notice}</p>
              <button type="button" onClick={() => setNotice(null)}>
                Dismiss
              </button>
            </div>
          ) : null}

          <div id="connection" className="dashboard-anchor">
            <ApiKeyCard hasKey={hasKey} onChanged={() => {
              void api.apiKeyStatus().then(setHasKey).catch((err) => setNotice(String(err)));
            }} />
          </div>

          <div id="loadout" ref={loadoutAnchor} className="dashboard-anchor">
            <fieldset disabled={!loadoutReady} style={{ border: 0, margin: 0, padding: 0, minWidth: 0 }}>
              <LoadoutSection
                loadout={loadout}
                onChange={setLoadout}
                onNotice={(message) => {
                  if (message) setNotice(message);
                }}
              />
            </fieldset>
            {loaded && !loadoutReady ? (
              <p role="status">Loadout could not be loaded. Restart Murmur to try again.</p>
            ) : null}
          </div>

          <div className="dashboard-work-grid">
            <section id="hotkeys" className="dashboard-surface dashboard-anchor">
              <div className="dashboard-section-heading">
                <div>
                  <h2>Shortcuts</h2>
                  <p>Use a key chord to start a task without leaving your current app.</p>
                </div>
                <button
                  type="button"
                  onClick={() => void testDictation()}
                  disabled={testing}
                  className="dashboard-subtle-button"
                >
                  {testing ? "Recording…" : "Test"}
                </button>
              </div>

              <ul className="dashboard-shortcut-list">
                {bindings.length === 0 ? (
                  <li className="dashboard-empty-state">
                    No shortcuts are active. Open Loadout to assign a key chord.
                  </li>
                ) : null}
                {bindings.map((binding) => {
                  const copy = GESTURE_COPY[binding.gesture];
                  return (
                    <li key={binding.gesture} className="dashboard-shortcut-item">
                      <div className="dashboard-key-combo">
                        <KeyCombo keys={keysFor(binding)} />
                      </div>
                      <div className="min-w-0">
                        <p>{copy.title}</p>
                        <span>{copy.detail}</span>
                      </div>
                    </li>
                  );
                })}
              </ul>
              <p className="dashboard-section-note">
                {loadout.hotkeys.hold_to_talk
                  ? "Hold-to-talk is on. Recording starts on press and sends when you release."
                  : "Tap-to-talk is on. Press once to start and again to stop."}
              </p>
            </section>

            <section id="scheduled" className="dashboard-surface dashboard-anchor">
              <div className="dashboard-section-heading">
                <div>
                  <h2>Scheduled</h2>
                  <p>Reminders and tasks you asked Murmur to handle.</p>
                </div>
                <span className="dashboard-count" aria-hidden="true">
                  {loaded ? activeTasks : "…"}
                </span>
                <span className="sr-only" aria-live="polite">
                  {loaded ? `${activeTasks} active tasks` : "Loading active tasks"}
                </span>
              </div>
              <div className="mt-4">
                <TaskList
                  tasks={tasks}
                  onPause={(id) => void pauseTask(id)}
                  onResume={(id) => void resumeTask(id)}
                  onDelete={(id) => void deleteTask(id)}
                  onSave={saveTask}
                />
              </div>
            </section>
          </div>

          <div id="diagnostics" className="dashboard-anchor">
            <DebugPanel bindings={bindings} />
          </div>

          <UpdateCard />

          <section id="notch-preview" className="dashboard-surface dashboard-anchor">
            <div className="dashboard-section-heading">
              <div>
                <h2>Notch preview</h2>
                <p>Preview the overlay states shown at the top of your screen.</p>
              </div>
              <MurmurMark size={18} />
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              {NOTCH_PREVIEWS.map((preview) => (
                <button
                  key={preview.value}
                  type="button"
                  onClick={() => void previewNotch(preview.value)}
                  className="dashboard-preview-button"
                >
                  {preview.label}
                </button>
              ))}
            </div>
          </section>

          <footer className="dashboard-privacy">
            <span className="dashboard-privacy-mark">
              <CheckIcon size={13} />
            </span>
            <p>
              Audio and screenshots stay in memory and go straight to Gemini. Nothing is recorded
              to disk. Tasks and preferences are stored locally.
            </p>
          </footer>
        </div>
      </main>
    </div>
  );
}
