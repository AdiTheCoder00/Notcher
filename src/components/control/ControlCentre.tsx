import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import {
  api,
  isTauri,
  type ControlSnapshot,
  type InstalledApp,
  type Loadout,
  type ScheduledTask,
} from "../../lib/ipc";
import { defaultLoadout } from "../../lib/theme";
import { EVENTS, useRustEvent } from "../../lib/bus";
import { MurmurMark, MuteIcon, NextIcon, PauseIcon, PlayIcon, PreviousIcon, VolumeIcon } from "../Icons";
import { ClipboardPanel, ShelfPanel, StatsPanel, WeatherPanel } from "./ExtraPanels";

interface Props {
  onClose: () => void;
  compact?: boolean;
  /** The hover preview: the same Now playing card, on its own. */
  peek?: boolean;
  active?: boolean;
}

/** The Now playing card, shared by the full panel and the hover preview. */
function NowPlayingPanel({
  media,
  snapshot,
  position,
  duration,
  progress,
  run,
}: {
  media: ControlSnapshot["media"];
  snapshot: ControlSnapshot | null;
  position: number | null;
  duration: number | null;
  progress: number;
  run: (action: () => Promise<unknown>) => Promise<void>;
}) {
  return (
    <section className="cc-media-panel">
      <div className="cc-panel-title">
        <span>Now playing</span>
        <span className="cc-muted">{media?.playing ? "Playing" : "Paused"}</span>
      </div>
      <div className="cc-track">
        <div className="cc-album-mark">{media?.artwork ? <img src={media.artwork} alt="" /> : <MurmurMark size={30} />}</div>
        <div className="cc-track-copy">
          <strong>{media?.title || (snapshot ? "Nothing playing" : "Connecting to media…")}</strong>
          <span>{media ? media.artist || "Unknown artist" : "Start playback to see track details"}</span>
        </div>
      </div>
      <div className="cc-progress-row">
        <span>{formatTime(position)}</span>
        <div className="cc-progress" role="progressbar" aria-label="Track progress" aria-valuemin={0} aria-valuemax={duration ?? 0} aria-valuenow={position ?? 0}>
          <span style={{ width: `${progress}%` }} />
        </div>
        <span>{formatTime(duration)}</span>
      </div>
      <div className="cc-player-actions">
        <button type="button" onClick={() => void run(api.mediaPrevious)} disabled={!media || !isTauri()} aria-label="Previous track"><PreviousIcon size={17} /></button>
        <button type="button" className="cc-play" onClick={() => void run(api.mediaToggle)} disabled={!media || !isTauri()} aria-label={media?.playing ? "Pause" : "Play"}>
          {media?.playing ? <PauseIcon size={16} /> : <PlayIcon size={16} />}
        </button>
        <button type="button" onClick={() => void run(api.mediaNext)} disabled={!media || !isTauri()} aria-label="Next track"><NextIcon size={17} /></button>
      </div>
    </section>
  );
}

type Panel = "control" | "today" | "stats" | "clipboard" | "shelf" | "apps" | "wallpapers" | "power";
type Backdrop = "terraces" | "midnight" | "forest" | "dusk";
type PowerAction = "lock" | "sleep" | "sign_out" | "restart" | "shutdown";

const DEMO_SNAPSHOT: ControlSnapshot = {
  media: {
    title: "lofi beats to study and relax",
    artist: "lofi radio",
    album: "",
    playing: true,
    position_seconds: 128,
    duration_seconds: 3725,
    artwork: null,
  },
  volume: 0.62,
  muted: false,
  volume_supported: true,
  wifi: { supported: true, enabled: true },
  bluetooth: { supported: true, enabled: false },
  dark_mode: true,
};

const BACKDROPS: { id: Backdrop; label: string; preview: string }[] = [
  {
    id: "terraces",
    label: "Terraced fields",
    preview: "url('/images/terraced-fields.jpg') center / cover",
  },
  {
    id: "midnight",
    label: "Midnight",
    preview: "linear-gradient(145deg, #303f60, #141922 68%)",
  },
  {
    id: "forest",
    label: "Forest",
    preview: "linear-gradient(145deg, #476255, #17241f 72%)",
  },
  {
    id: "dusk",
    label: "Dusk",
    preview: "linear-gradient(145deg, #765a69, #252432 72%)",
  },
];

const REFRESH_MS = 2500;
const TIMER_PRESETS = [1, 5, 10, 15, 20];
const RECENT_APPS_KEY = "murmur-recent-apps";

/** The app's own icon when Windows gave us one, otherwise its initial. */
function AppMark({ app }: { app: InstalledApp }) {
  if (app.icon) {
    return <img className="cc-app-mark cc-app-icon" src={app.icon} alt="" aria-hidden="true" />;
  }
  return (
    <span className="cc-app-mark" aria-hidden="true">
      {app.name.slice(0, 1).toUpperCase()}
    </span>
  );
}
type Todo = { id: number; text: string; done: boolean };
function readTodos(): Todo[] {
  try { const value: unknown = JSON.parse(localStorage.getItem("murmur-todos") || "[]"); return Array.isArray(value) ? value.filter((item): item is Todo => typeof item?.id === "number" && typeof item?.text === "string" && typeof item?.done === "boolean") : []; } catch { return []; }
}

function readRecentApps(): string[] {
  try {
    const stored = window.localStorage.getItem(RECENT_APPS_KEY);
    if (!stored) return [];
    const parsed: unknown = JSON.parse(stored);
    if (!Array.isArray(parsed) || !parsed.every((name) => typeof name === "string")) {
      throw new Error("Saved recent apps are not a list of names.");
    }
    return parsed.slice(0, 8);
  } catch (err) {
    console.error("Could not read recently opened apps:", err);
    return [];
  }
}

function formatTime(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "--:--";
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.floor(seconds % 60);
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}

function sameDay(left: Date, right: Date): boolean {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function taskTime(task: ScheduledTask): string {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(
    new Date(task.due_at_ms),
  );
}

export function ControlCentre({ onClose, compact = false, peek = false, active = true }: Props) {
  const [snapshot, setSnapshot] = useState<ControlSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel>("control");
  const [loadout, setLoadout] = useState<Loadout>(defaultLoadout());
  const [installedApps, setInstalledApps] = useState<InstalledApp[]>([]);
  const [appsLoading, setAppsLoading] = useState(false);
  const [appSearch, setAppSearch] = useState("");
  const [recentAppNames, setRecentAppNames] = useState<string[]>(readRecentApps);
  const appSearchRef = useRef<HTMLInputElement>(null);
  const appListRef = useRef<HTMLDivElement>(null);
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  const [todos, setTodos] = useState<Todo[]>(readTodos);
  const [todoDraft, setTodoDraft] = useState("");
  const [backdrop, setBackdrop] = useState<Backdrop | null>(null);
  const [now, setNow] = useState(() => new Date());
  const [timerMinutes, setTimerMinutes] = useState(5);
  const [timerRemaining, setTimerRemaining] = useState(0);
  const [timerRunning, setTimerRunning] = useState(false);
  const [timerComplete, setTimerComplete] = useState(false);
  const timerDeadline = useRef(0);
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null);
  const volumeTimer = useRef<number | null>(null);
  const reducedMotion = useReducedMotion();
  const pickerOpen = useRef(false);

  useEffect(() => () => { if (volumeTimer.current !== null) window.clearTimeout(volumeTimer.current); }, []);

  useEffect(() => { if (!active) setPanel("control"); }, [active]);
  useEffect(() => {
    if (active && !compact && !peek && isTauri()) {
      void invoke("dashboard_resize_panel", { panel }).catch(err => setError(String(err)));
    }
  }, [active, compact, peek, panel]);

  useEffect(() => { try { localStorage.setItem("murmur-todos", JSON.stringify(todos)); } catch (err) { setError(`Could not save to-dos: ${String(err)}`); } }, [todos]);

  useRustEvent(EVENTS.loadoutChanged, setLoadout);
  useRustEvent(EVENTS.taskDue, () => {
    if (isTauri()) void api.listTasks().then(setTasks).catch((err) => setError(String(err)));
  });

  useEffect(() => {
    if (active && isTauri()) void api.listTasks().then(setTasks).catch((err) => setError(String(err)));
  }, [active]);

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setSnapshot(DEMO_SNAPSHOT);
      return;
    }

    try {
      setSnapshot(await api.controlSnapshot());
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [active, refresh]);

  useEffect(() => {
    if (!isTauri()) {
      setLoadout(defaultLoadout());
      setTasks([]);
      return;
    }

    let cancelled = false;
    Promise.all([api.getLoadout(), api.listTasks()])
      .then(([savedLoadout, savedTasks]) => {
        if (cancelled) return;
        setLoadout(savedLoadout);
        setTasks(savedTasks);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(String(err));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!active || panel !== "apps" || !isTauri()) return;

    let cancelled = false;
    setAppsLoading(true);
    api
      .listInstalledApps()
      .then((apps) => {
        if (!cancelled) setInstalledApps(apps);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setAppsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [active, panel]);

  useEffect(() => {
    if (!active || panel !== "apps") return;
    appSearchRef.current?.focus();
  }, [active, panel]);

  useEffect(() => {
    try {
      window.localStorage.setItem(RECENT_APPS_KEY, JSON.stringify(recentAppNames));
    } catch (err) {
      console.error("Could not save recently opened apps:", err);
      setError(`Could not save recently opened apps: ${String(err)}`);
    }
  }, [recentAppNames]);

  useEffect(() => {
    const clock = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(clock);
  }, []);

  useEffect(() => {
    if (!timerRunning) return;
    const timer = window.setInterval(() => {
      const remaining = Math.max(0, Math.ceil((timerDeadline.current - Date.now()) / 1000));
      setTimerRemaining(remaining);
      if (remaining === 0) {
        setTimerRunning(false);
        setTimerComplete(true);
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [timerRunning]);

  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setError(null);
      try {
        await action();
        await refresh();
      } catch (err) {
        setError(String(err));
      }
    },
    [refresh],
  );

  const media = snapshot?.media ?? null;
  const duration = media?.duration_seconds ?? null;
  const position = media?.position_seconds ?? null;
  const progress =
    duration && duration > 0 && position !== null
      ? Math.min(100, Math.max(0, (position / duration) * 100))
      : 0;
  const todayTasks = useMemo(
    () =>
      tasks
        .filter(
          (task) =>
            (task.status === "pending" || task.status === "running") &&
            sameDay(new Date(task.due_at_ms), now),
        )
        .sort((left, right) => left.due_at_ms - right.due_at_ms),
    [now, tasks],
  );
  const filteredApps = useMemo(() => {
    const query = appSearch.trim().toLocaleLowerCase();
    return installedApps.filter((app) => app.name.toLocaleLowerCase().includes(query));
  }, [appSearch, installedApps]);
  const recentApps = recentAppNames
    .map((name) =>
      installedApps.find((app) => app.name.toLocaleLowerCase() === name.toLocaleLowerCase()),
    )
    .filter((app): app is InstalledApp => app !== undefined)
    .filter((app) => filteredApps.includes(app));
  const starredApps = filteredApps.filter((app) =>
    loadout.apps.pinned.some((name) => name.toLocaleLowerCase() === app.name.toLocaleLowerCase()),
  );
  const otherApps = filteredApps.filter(
    (app) =>
      !loadout.apps.pinned.some((name) => name.toLocaleLowerCase() === app.name.toLocaleLowerCase()),
  );

  const openInstalledApp = useCallback(async (app: InstalledApp) => {
    setError(null);
    try {
      await api.launchInstalledApp(app.target);
      setRecentAppNames((recent) =>
        [app.name, ...recent.filter((name) => name.toLocaleLowerCase() !== app.name.toLocaleLowerCase())].slice(0, 8),
      );
      onClose();
    } catch (err) {
      setError(String(err));
    }
  }, [onClose]);

  const togglePinnedApp = useCallback(
    async (app: InstalledApp) => {
      const pinned = loadout.apps.pinned;
      const isPinned = pinned.some((name) => name.toLocaleLowerCase() === app.name.toLocaleLowerCase());
      if (!isPinned && pinned.length >= 12) {
        setError("A Loadout can pin at most 12 apps.");
        return;
      }

      setError(null);
      try {
        const saved = await api.patchLoadout({
          apps: {
            pinned: isPinned
              ? pinned.filter((name) => name.toLocaleLowerCase() !== app.name.toLocaleLowerCase())
              : [...pinned, app.name],
          },
        });
        setLoadout(saved);
      } catch (err) {
        setError(String(err));
      }
    },
    [loadout],
  );

  const enterLauncher = useCallback(() => {
    api
      .openLauncher()
      .then(() => window.requestAnimationFrame(() => appSearchRef.current?.focus()))
      .catch((err) => setError(String(err)));
  }, []);
  const calendarDays = useMemo(() => {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const mondayOffset = (start.getDay() + 6) % 7;
    start.setDate(start.getDate() - mondayOffset);
    return Array.from({ length: 7 }, (_, index) => {
      const date = new Date(start);
      date.setDate(start.getDate() + index);
      return date;
    });
  }, [now]);

  // Move the thumb immediately and push the level to Windows on a short debounce,
  // so dragging feels live and a release outside the panel still commits.
  const setVolume = (level: number) => {
    const next = Math.min(1, Math.max(0, level));
    setVolumeDraft(next);
    setSnapshot((current) => (current ? { ...current, volume: next, muted: next > 0 ? false : current.muted } : current));
    if (volumeTimer.current !== null) window.clearTimeout(volumeTimer.current);
    volumeTimer.current = window.setTimeout(() => {
      volumeTimer.current = null;
      void run(() => api.setVolume(next)).finally(() => setVolumeDraft((value) => value === next ? null : value));
    }, 90);
  };

  const invokePower = (action: PowerAction) => {
    const confirmation: Partial<Record<PowerAction, string>> = {
      sign_out: "Sign out of Windows? Save your work before continuing.",
      restart: "Restart this PC? Save your work before continuing.",
      shutdown: "Shut down this PC? Save your work before continuing.",
    };
    const message = confirmation[action];
    if (message && !window.confirm(message)) return;
    void run(() => api.powerAction(action));
  };

  const selectWallpaper = (preset: Backdrop) => {
    if (!isTauri()) {
      setError("Changing the Windows wallpaper requires the desktop app.");
      return;
    }
    void run(async () => {
      await api.setWallpaper(preset);
      setBackdrop(preset);
    });
  };

  const tabs: { id: Panel; label: string }[] = [
    { id: "control", label: "Controls" },
    { id: "today", label: "Today" },
    { id: "stats", label: "Stats" },
    { id: "clipboard", label: "Clipboard" },
    { id: "shelf", label: "Shelf" },
    { id: "apps", label: "Apps" },
    { id: "wallpapers", label: "Wallpaper" },
    { id: "power", label: "Power" },
  ];

  const handleLauncherKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      setPanel("apps");
      if (isTauri()) enterLauncher();
      appSearchRef.current?.focus();
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      if (event.target === appSearchRef.current) {
        if (appSearch) setAppSearch("");
        else onClose();
      } else {
        onClose();
      }
      return;
    }

    if (panel !== "apps" || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) return;
    const buttons = [...(appListRef.current?.querySelectorAll<HTMLButtonElement>(".cc-app-open") ?? [])];
    if (!buttons.length) return;

    event.preventDefault();
    const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex =
      currentIndex < 0
        ? event.key === "ArrowDown" ? 0 : buttons.length - 1
        : (currentIndex + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
    buttons[nextIndex]?.focus();
  };

  if (peek) {
    return (
      <motion.section
        initial={{ opacity: 0, y: reducedMotion ? 0 : -8, scale: reducedMotion ? 1 : 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: reducedMotion ? 0 : -5, scale: reducedMotion ? 1 : 0.99 }}
        transition={
          reducedMotion
            ? { duration: 0.12 }
            : { type: "spring", stiffness: 380, damping: 34, mass: 0.8 }
        }
        className="cc-peek"
        aria-label="Now playing"
      >
        <NowPlayingPanel
          media={media}
          snapshot={snapshot}
          position={position}
          duration={duration}
          progress={progress}
          run={run}
        />
      </motion.section>
    );
  }

  if (compact) {
    return (
      <motion.section
        initial={{ opacity: 0, y: reducedMotion ? 0 : -8, scale: reducedMotion ? 1 : 0.97 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{
          opacity: 0,
          y: reducedMotion ? 0 : -5,
          scale: reducedMotion ? 1 : 0.985,
        }}
        transition={
          reducedMotion
            ? { duration: 0.12 }
            : { type: "spring", stiffness: 360, damping: 32, mass: 0.8 }
        }
        className="relative flex h-full w-full items-start justify-center overflow-hidden px-2 py-1"
        style={{ color: "var(--notch-text)" }}
        aria-label="Media controls"
        onMouseLeave={onClose}
      >
        <div
          className="relative w-full overflow-hidden rounded-[22px] border border-white/15"
          style={{
            background:
              "linear-gradient(135deg, rgba(20, 25, 34, 0.76), rgba(11, 14, 20, 0.72))",
            boxShadow:
              "inset 0 1px 0 rgba(255,255,255,0.12), 0 14px 34px rgba(0,0,0,0.24)",
            backdropFilter: "blur(32px) saturate(145%)",
            WebkitBackdropFilter: "blur(32px) saturate(145%)",
          }}
        >
          <div className="relative flex items-center gap-2.5 px-3 pb-2 pt-2.5">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px] border border-white/15 bg-slate-700/80">
              <span className="text-white/90">
                <MurmurMark size={21} />
              </span>
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-[12px] font-semibold leading-tight text-white/95">
                {media?.title || (snapshot ? "Nothing playing" : "Connecting to media…")}
              </p>
              <p className="mt-0.5 truncate text-[11px] leading-tight text-white/80">
                {media
                  ? [media.artist, media.album].filter(Boolean).join(" · ") || "Unknown artist"
                  : "Start playback to see track details"}
              </p>
            </div>
          </div>

          <div className="relative flex items-center gap-2 px-3 pb-1.5">
            <span className="w-9 shrink-0 text-[11px] tabular-nums text-white/80">
              {formatTime(position)}
            </span>
            <div
              className="h-[3px] flex-1 overflow-hidden rounded-full bg-white/20"
              role="progressbar"
              aria-label="Track progress"
              aria-valuemin={0}
              aria-valuemax={duration ?? 0}
              aria-valuenow={position ?? 0}
            >
              <div className="h-full rounded-full bg-sky-200" style={{ width: `${progress}%` }} />
            </div>
            <span className="w-9 shrink-0 text-right text-[11px] tabular-nums text-white/80">
              {formatTime(duration)}
            </span>
          </div>

          <div className="relative flex items-center justify-center gap-2 border-t border-white/[0.08] px-3 py-1.5">
            <button
              type="button"
              onClick={() => void run(api.mediaPrevious)}
              disabled={!media || !isTauri()}
              className="flex h-7 min-w-7 items-center justify-center rounded-full p-1 text-white/80 transition hover:bg-white/10 hover:text-white active:scale-95 disabled:opacity-30"
              aria-label="Previous track"
            >
              <PreviousIcon size={12} />
            </button>
            <button
              type="button"
              onClick={() => void run(api.mediaToggle)}
              disabled={!media || !isTauri()}
              className="flex min-h-7 min-w-7 items-center justify-center rounded-[10px] border border-sky-100/25 bg-sky-200 px-3.5 py-1 text-slate-950 transition hover:bg-sky-100 active:scale-95 disabled:opacity-30"
              aria-label={media?.playing ? "Pause" : "Play"}
            >
              {media?.playing ? <PauseIcon size={13} /> : <PlayIcon size={13} />}
            </button>
            <button
              type="button"
              onClick={() => void run(api.mediaNext)}
              disabled={!media || !isTauri()}
              className="flex h-7 min-w-7 items-center justify-center rounded-full p-1 text-white/80 transition hover:bg-white/10 hover:text-white active:scale-95 disabled:opacity-30"
              aria-label="Next track"
            >
              <NextIcon size={12} />
            </button>
          </div>
        </div>
      </motion.section>
    );
  }

  return (
    <motion.section
      initial={{ opacity: 0, y: reducedMotion ? 0 : -8, scale: reducedMotion ? 1 : 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: reducedMotion ? 0 : -5, scale: reducedMotion ? 1 : 0.99 }}
      transition={
        reducedMotion
          ? { duration: 0.12 }
          : { type: "spring", stiffness: 340, damping: 32, mass: 0.85 }
      }
      className="cc-overlay"
      aria-label="Control Centre"
      onMouseLeave={() => { if (!pickerOpen.current) onClose(); }}
      onKeyDown={handleLauncherKeyDown}
    >
      <header className="cc-header">
        <nav className="cc-tabs" aria-label="Control Centre panels">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`cc-tab${panel === tab.id ? " is-active" : ""}`}
              aria-current={panel === tab.id ? "page" : undefined}
              onClick={() => {
                setPanel(tab.id);
                if ((tab.id === "apps" || tab.id === "today" || tab.id === "clipboard" || tab.id === "shelf") && isTauri()) {
                  enterLauncher();
                }
              }}
            >
              {tab.label}
            </button>
          ))}
        </nav>
        <button type="button" className="cc-close" onClick={onClose} aria-label="Close Control Centre">
          ×
        </button>
      </header>

      <div className="cc-body">
        {panel === "stats" ? <StatsPanel /> : null}
        <div className="cc-persistent-panel" hidden={panel !== "clipboard"}><ClipboardPanel /></div>
        {panel === "shelf" ? <ShelfPanel onPicker={(busy) => { pickerOpen.current = busy; }} /> : null}
        {panel === "control" ? (
          <motion.div
            key="control"
            className="cc-control-grid"
            initial={{ opacity: 0, y: reducedMotion ? 0 : 7 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: reducedMotion ? 0 : 0.18 }}
          >
            <NowPlayingPanel
              media={media}
              snapshot={snapshot}
              position={position}
              duration={duration}
              progress={progress}
              run={run}
            />

            <section className="cc-system-panel">
              <div className="cc-panel-title">
                <span>System controls</span>
                <span className="cc-muted">This PC</span>
              </div>
              <div className="cc-toggle-grid">
                <button type="button" className={`cc-toggle${snapshot?.wifi.enabled ? " is-on" : ""}`} onClick={() => void run(() => api.setRadioEnabled("wifi", !snapshot?.wifi.enabled))} disabled={!isTauri() || !snapshot?.wifi.supported}>
                  <span className="cc-toggle-symbol">Wi</span><span><strong>Wi-Fi</strong><small>{snapshot?.wifi.supported ? snapshot.wifi.enabled ? "On" : "Off" : "Unavailable"}</small></span>
                </button>
                <button type="button" className={`cc-toggle${snapshot?.bluetooth.enabled ? " is-on" : ""}`} onClick={() => void run(() => api.setRadioEnabled("bluetooth", !snapshot?.bluetooth.enabled))} disabled={!isTauri() || !snapshot?.bluetooth.supported}>
                  <span className="cc-toggle-symbol">BT</span><span><strong>Bluetooth</strong><small>{snapshot?.bluetooth.supported ? snapshot.bluetooth.enabled ? "On" : "Off" : "Unavailable"}</small></span>
                </button>
                <button type="button" className={`cc-toggle${snapshot?.volume_supported && !snapshot.muted ? " is-on" : ""}`} onClick={() => void run(() => api.setMuted(!snapshot?.muted))} disabled={!isTauri() || !snapshot?.volume_supported}>
                  <span className="cc-toggle-symbol">♪</span><span><strong>Audio</strong><small>{snapshot?.muted ? "Muted" : "Speakers"}</small></span>
                </button>
                <button type="button" className="cc-toggle" onClick={() => setPanel("power")}><span className="cc-toggle-symbol">⏻</span><span><strong>Power</strong><small>Power options</small></span></button>
                <button type="button" className="cc-toggle" disabled><span className="cc-toggle-symbol">☾</span><span><strong>Night Light</strong><small>Unavailable</small></span></button>
                <button type="button" className={`cc-toggle${snapshot?.dark_mode ? " is-on" : ""}`} onClick={() => void run(() => api.setDarkMode(!snapshot?.dark_mode))} disabled={!isTauri()}>
                  <span className="cc-toggle-symbol">◐</span><span><strong>Dark mode</strong><small>{snapshot?.dark_mode ? "On" : "Off"}</small></span>
                </button>
              </div>
              <div className="cc-volume-card">
                <span className="cc-volume-icon" aria-hidden="true">
                  {snapshot?.muted ? <MuteIcon size={15} /> : <VolumeIcon size={15} />}
                </span>
                <span className="cc-volume-label">Volume</span>
                <input
                  className="cc-range"
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  value={volumeDraft ?? snapshot?.volume ?? 0}
                  disabled={!isTauri() || !snapshot?.volume_supported}
                  onChange={(event) => setVolume(Number(event.currentTarget.value))}
                  style={{ "--cc-fill": `${Math.round((volumeDraft ?? snapshot?.volume ?? 0) * 100)}%` } as CSSProperties}
                  aria-label="System volume"
                />
                <output>{Math.round((volumeDraft ?? snapshot?.volume ?? 0) * 100)}%</output>
              </div>
            </section>
          </motion.div>
        ) : null}

        {panel === "today" ? (
          <motion.div
            key="today"
            className="cc-today-grid"
            initial={{ opacity: 0, y: reducedMotion ? 0 : 7 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: reducedMotion ? 0 : 0.18 }}
          >
            <section className="cc-date-panel">
              <div className="cc-clock">
                <strong>{String(now.getHours()).padStart(2, "0")}</strong>
                <span>•••</span>
                <strong>{String(now.getMinutes()).padStart(2, "0")}</strong>
                <small>{new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric" }).format(now)}</small>
              </div>
              <WeatherPanel />
              <div className="cc-calendar">
              <div className="cc-panel-title"><span>This week</span><span className="cc-muted">{new Intl.DateTimeFormat(undefined, { month: "short", year: "numeric" }).format(now)}</span></div>
              <div className="cc-week-grid" aria-label="This week's calendar">
                {calendarDays.map((date) => (
                  <div key={date.toISOString()} className={`cc-day${sameDay(date, now) ? " is-today" : ""}`}>
                    <span>{new Intl.DateTimeFormat(undefined, { weekday: "narrow" }).format(date)}</span>
                    <strong>{date.getDate()}</strong>
                  </div>
                ))}
              </div>
              <div className="cc-next-task">{todayTasks[0] ? `Next · ${taskTime(todayTasks[0])} — ${todayTasks[0].prompt}` : "No more tasks today"}</div>
              </div>
              <div className="cc-timer">
                <div>
                  <span className="cc-panel-title">Focus timer</span>
                  <strong className="cc-timer-value">{timerComplete ? "Done" : formatTime(timerRemaining || timerMinutes * 60)}</strong>
                </div>
                <div className="cc-timer-presets">
                  {TIMER_PRESETS.map((minutes) => (
                    <button key={minutes} type="button" className={timerMinutes === minutes ? "is-selected" : ""} onClick={() => { setTimerMinutes(minutes); setTimerRemaining(0); setTimerRunning(false); setTimerComplete(false); }}>
                      {minutes}m
                    </button>
                  ))}
                </div>
                <div className="cc-timer-actions">
                  <button type="button" className="cc-action-primary" onClick={() => {
                    if (timerRunning) {
                      const remaining = Math.max(0, Math.ceil((timerDeadline.current - Date.now()) / 1000));
                      setTimerRemaining(remaining);
                      setTimerComplete(remaining === 0);
                      setTimerRunning(false);
                    }
                    else {
                      timerDeadline.current = Date.now() + (timerRemaining || timerMinutes * 60) * 1000;
                      if (timerRemaining === 0) {
                        setTimerRemaining(timerMinutes * 60);
                        setTimerComplete(false);
                      }
                      setTimerRunning(true);
                    }
                  }}>
                    {timerRunning ? "Pause" : timerRemaining > 0 ? "Resume" : "Start"}
                  </button>
                  <button type="button" className="cc-action-quiet" onClick={() => { setTimerRunning(false); setTimerRemaining(0); setTimerComplete(false); }}>Reset</button>
                </div>
              </div>
            </section>

            <section className="cc-agenda-panel">
              <div className="cc-panel-title">
                <span>Today</span>
                <span className="cc-muted">{todos.filter(todo => todo.done).length}/{todos.length}</span>
              </div>
              <ul className="cc-todo-list">{todos.map(todo => <li key={todo.id}><label><input type="checkbox" checked={todo.done} onChange={() => setTodos(old => old.map(item => item.id === todo.id ? { ...item, done: !item.done } : item))} /><span className={todo.done ? "is-done" : ""}>{todo.text}</span></label><button type="button" aria-label={`Remove ${todo.text}`} onClick={() => setTodos(old => old.filter(item => item.id !== todo.id))}>×</button></li>)}</ul>
              {todayTasks.length ? (
                <ul className="cc-agenda-list">
                  {todayTasks.map((task) => (
                    <li key={task.id}>
                      <time>{taskTime(task)}</time>
                      <span>{task.prompt}</span>
                    </li>
                  ))}
                </ul>
              ) : null}
              <form className="cc-todo-form" onSubmit={(event) => { event.preventDefault(); const text = todoDraft.trim(); if (text) { setTodos(old => [...old, { id: Date.now(), text, done: false }]); setTodoDraft(""); } }}>
                <input value={todoDraft} onFocus={() => { if (isTauri()) enterLauncher(); }} onChange={event => setTodoDraft(event.target.value)} placeholder="＋  Add a to-do..." aria-label="Add a to-do" /><button type="submit" aria-label="Add to-do">→</button>
              </form>
            </section>
          </motion.div>
        ) : null}

        {panel === "apps" ? (
          <motion.section key="apps" className="cc-apps-panel" initial={{ opacity: 0, y: reducedMotion ? 0 : 7 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.18 }}>
            <label className="cc-app-search">
              <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.5" /><path d="m13 13 4 4" /></svg>
              <input
                ref={appSearchRef}
                type="search"
                value={appSearch}
                onChange={(event) => setAppSearch(event.currentTarget.value)}
                placeholder="Search apps..."
                aria-label="Search installed apps"
                onKeyDown={(event) => {
                  if (event.key === "Enter" && filteredApps[0]) {
                    event.preventDefault();
                    void openInstalledApp(filteredApps[0]);
                  }
                }}
              />
              <span className="cc-app-search-shortcut">Ctrl K</span>
            </label>

            {!isTauri() ? (
              <p className="cc-app-empty">Open the Murmur desktop app to browse installed apps.</p>
            ) : appsLoading ? (
              <p className="cc-app-empty" role="status">Loading installed apps…</p>
            ) : filteredApps.length ? (
              <div className="cc-app-list" ref={appListRef} aria-label="Installed apps">
                {recentApps.length ? (
                  <section className="cc-app-group">
                    <h3>Recent</h3>
                    {recentApps.map((app) => (
                      <div className="cc-app-row" key={`recent-${app.target}`}>
                        <button type="button" className="cc-app-open" onClick={() => void openInstalledApp(app)}>
                          <AppMark app={app} />
                          <strong>{app.name}</strong>
                          <span>Recently opened</span>
                          <span className="cc-app-open-label">Open</span>
                        </button>
                      </div>
                    ))}
                  </section>
                ) : null}
                {starredApps.length ? (
                  <section className="cc-app-group">
                    <h3>Starred</h3>
                    {starredApps.map((app) => (
                      <div className="cc-app-row" key={app.target}>
                        <button type="button" className="cc-app-open" onClick={() => void openInstalledApp(app)}>
                          <AppMark app={app} />
                          <strong>{app.name}</strong>
                          <span>Application</span>
                          <span className="cc-app-open-label">Open</span>
                        </button>
                        <button type="button" className="cc-app-star is-starred" onClick={() => void togglePinnedApp(app)} aria-label={`Remove ${app.name} from starred apps`} title="Remove from starred apps">
                          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m10 1.8 2.5 5.1 5.7.8-4.1 4 .97 5.7L10 14.7l-5.1 2.7.97-5.7-4.1-4 5.7-.8L10 1.8Z" /></svg>
                        </button>
                      </div>
                    ))}
                  </section>
                ) : null}
                <section className="cc-app-group">
                  <h3>{appSearch ? "Results" : "Applications"}</h3>
                  {otherApps.length ? otherApps.map((app) => (
                    <div className="cc-app-row" key={app.target}>
                      <button type="button" className="cc-app-open" onClick={() => void openInstalledApp(app)}>
                        <AppMark app={app} />
                        <strong>{app.name}</strong>
                        <span>Application</span>
                        <span className="cc-app-open-label">Open</span>
                      </button>
                      <button type="button" className="cc-app-star" onClick={() => void togglePinnedApp(app)} aria-label={`Add ${app.name} to starred apps`} title="Add to starred apps">
                        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m10 1.8 2.5 5.1 5.7.8-4.1 4 .97 5.7L10 14.7l-5.1 2.7.97-5.7-4.1-4 5.7-.8L10 1.8Z" /></svg>
                      </button>
                    </div>
                  )) : !starredApps.length ? <p className="cc-app-empty">No apps match “{appSearch}”.</p> : null}
                </section>
              </div>
            ) : (
              <p className="cc-app-empty" role="status">No installed Start Menu apps were found.</p>
            )}
          </motion.section>
        ) : null}

        {panel === "wallpapers" ? (
          <motion.section key="wallpapers" className="cc-wallpapers-panel" initial={{ opacity: 0, y: reducedMotion ? 0 : 7 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.18 }}>
            <div className="cc-panel-title">
              <span><button type="button" className="cc-wallpaper-folder" disabled={!isTauri()} onClick={() => { pickerOpen.current = true; void invoke<string | null>("choose_wallpaper").then(path => { if (path) setBackdrop(null); }).catch(err => setError(String(err))).finally(() => { pickerOpen.current = false; }); }}>My folder</button></span>
              <span className="cc-muted">Sets your desktop background</span>
            </div>
            <div className="cc-wallpaper-grid">
              {BACKDROPS.map((item) => (
                <button key={item.id} type="button" className={`cc-wallpaper-option${backdrop === item.id ? " is-selected" : ""}`} onClick={() => selectWallpaper(item.id)} aria-pressed={backdrop === item.id}>
                  <span className="cc-wallpaper-preview" style={{ background: item.preview }} />
                  <span>{item.label}</span>
                </button>
              ))}
            </div>
          </motion.section>
        ) : null}

        {panel === "power" ? (
          <motion.section key="power" className="cc-power-panel" initial={{ opacity: 0, y: reducedMotion ? 0 : 7 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.18 }}>
            <div className="cc-power-heading">
              <span className="cc-power-mark"><MurmurMark size={25} /></span>
              <div>
                <h2>Power options</h2>
                <p>Lock, sleep, or restart this Windows device.</p>
              </div>
            </div>
            <div className="cc-power-actions">
              {([
                ["lock", "Lock"],
                ["sleep", "Sleep"],
                ["restart", "Restart"],
                ["shutdown", "Shut down"],
              ] as const).map(([action, label]) => (
                <button key={action} type="button" className={action === "shutdown" ? "cc-power-button is-danger" : "cc-power-button"} disabled={!isTauri()} onClick={() => invokePower(action)}>
                  <svg className="cc-power-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">{action === "lock" ? <><rect x="5.5" y="10" width="13" height="10" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/></> : action === "sleep" ? <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5Z"/> : action === "restart" ? <><path d="M20 5v5h-5"/><path d="M19 10a7.5 7.5 0 1 0 1 5"/></> : <><path d="M12 2v10"/><path d="M7 5.5a8 8 0 1 0 10 0"/></>}</svg>
                  <strong>{label}</strong>
                </button>
              ))}
            </div>
          </motion.section>
        ) : null}
      </div>

      {!isTauri() ? <p className="cc-preview-note">System actions work in the Murmur desktop app.</p> : null}
      {error ? <p className="cc-error" role="alert" title={error}>{error}</p> : null}
    </motion.section>
  );
}
