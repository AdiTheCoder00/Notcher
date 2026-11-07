import { useEffect, useState } from "react";
import {
  api,
  isTauri,
  type AudioLevel,
  type GestureName,
  type HotkeyBinding,
  type HotkeySnapshot,
  type MicInfo,
} from "../lib/ipc";
import { EVENTS, useRustEvent } from "../lib/bus";

const GESTURE_LABEL: Record<GestureName, string> = {
  dictation: "Dictation",
  circle_to_ask: "Circle to ask",
  schedule_task: "Schedule by voice",
};

const IDLE_SNAPSHOT: HotkeySnapshot = {
  gesture: null,
  ctrl: false,
  shift: false,
  alt: false,
  win: false,
};

const IDLE_AUDIO: AudioLevel = { level: 0, recording: false, device: null, error: null };

const MODIFIERS: { key: keyof Omit<HotkeySnapshot, "gesture">; label: string }[] = [
  { key: "ctrl", label: "Ctrl" },
  { key: "shift", label: "Shift" },
  { key: "alt", label: "Alt" },
  { key: "win", label: "Win" },
];

function keysFor(binding: HotkeyBinding): string {
  const keys: string[] = [];
  if (binding.ctrl) keys.push("Ctrl");
  if (binding.shift) keys.push("Shift");
  if (binding.alt) keys.push("Alt");
  if (binding.win) keys.push("Win");
  return keys.join(" + ");
}

interface Props {
  bindings: HotkeyBinding[];
}

/**
 * Live view of the low-level hook and the capture meter, read through the typed
 * event bus. Useful while the gesture plumbing is being built.
 */
export function DebugPanel({ bindings }: Props) {
  const [snapshot, setSnapshot] = useState<HotkeySnapshot>(IDLE_SNAPSHOT);
  const [audio, setAudio] = useState<AudioLevel>(IDLE_AUDIO);
  const [mic, setMic] = useState<MicInfo | null>(null);

  useRustEvent(EVENTS.hotkeyState, setSnapshot);
  useRustEvent(EVENTS.audioLevel, setAudio);

  useEffect(() => {
    if (!isTauri()) return;
    api
      .micStatus()
      .then(setMic)
      .catch(() => undefined);
  }, []);

  const heldGesture = snapshot.gesture ? GESTURE_LABEL[snapshot.gesture] : "Idle";
  const percent = Math.round((audio.recording ? audio.level : 0) * 100);
  const deviceName = audio.device ?? mic?.device ?? null;
  const micError = audio.error ?? mic?.error ?? null;

  return (
    <section className="dashboard-surface rounded-2xl border border-line bg-panel/80 p-5">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold text-zinc-100">Hook debug</h2>
          <p className="mt-1 text-xs text-muted">
            Raw state from the keyboard hook and the capture meter.
          </p>
        </div>
        <span
          className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-medium ring-1 ring-inset ${
            audio.recording
              ? "bg-red-500/10 text-red-300 ring-red-500/25"
              : "bg-white/5 text-muted ring-white/10"
          }`}
        >
          <span
            className={`h-1.5 w-1.5 rounded-full ${
              audio.recording ? "bg-red-400" : "bg-zinc-500"
            }`}
          />
          {audio.recording ? "Recording" : "Idle"}
        </span>
      </div>

      <div className="mt-4 grid gap-4 sm:grid-cols-2">
        <div>
          <p className="text-[10px] font-medium uppercase tracking-widest text-muted">
            Modifiers held
          </p>
          <div className="mt-2 flex gap-1.5">
            {MODIFIERS.map((modifier) => (
              <span
                key={modifier.key}
                className={`rounded-md border px-2 py-1 font-mono text-[11px] transition ${
                  snapshot[modifier.key]
                    ? "border-accent/50 bg-accent/15 text-accent-soft"
                    : "border-line bg-panel-raised/50 text-muted"
                }`}
              >
                {modifier.label}
              </span>
            ))}
          </div>
          <p className="mt-2 text-[11px] text-muted">
            Combo:{" "}
            <span className={snapshot.gesture ? "text-emerald-300" : "text-zinc-300"}>
              {heldGesture}
            </span>
          </p>
        </div>

        <div>
          <div className="flex items-baseline justify-between">
            <p className="text-[10px] font-medium uppercase tracking-widest text-muted">
              Input level
            </p>
            <span className="font-mono text-[11px] tabular-nums text-muted">{percent}%</span>
          </div>
          <div className="mt-2 h-2.5 overflow-hidden rounded-full bg-white/5">
            <div
              className="h-full rounded-full bg-gradient-to-r from-emerald-400 via-cyan-300 to-indigo-400 transition-[width] duration-75 ease-out"
              style={{ width: `${Math.min(100, percent)}%` }}
            />
          </div>
          <p className="mt-2 truncate text-[11px] text-muted" title={deviceName ?? undefined}>
            {deviceName ?? "No microphone detected"}
          </p>
        </div>
      </div>

      {micError ? (
        <p className="mt-3 rounded-lg border border-red-500/25 bg-red-500/5 px-3 py-2 text-[11px] leading-relaxed text-red-300">
          {micError}
        </p>
      ) : null}

      <div className="mt-4 border-t border-line/70 pt-3">
        <p className="text-[10px] font-medium uppercase tracking-widest text-muted">
          Active bindings (config)
        </p>
        <ul className="mt-2 space-y-1">
          {bindings.map((binding) => (
            <li
              key={binding.gesture}
              className="flex items-center justify-between gap-3 text-[11px]"
            >
              <span className="text-zinc-300">{GESTURE_LABEL[binding.gesture]}</span>
              <span className="font-mono text-muted">{keysFor(binding)}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
