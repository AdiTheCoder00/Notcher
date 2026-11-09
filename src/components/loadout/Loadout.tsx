import { useCallback, useMemo, useRef, useState } from "react";
import type { ReactElement, ReactNode } from "react";
import { motion, useReducedMotion } from "framer-motion";
import type {
  GestureName,
  Loadout,
  PersonaPreset,
  ThemeMode,
  ThemePalette,
  VoiceStyle,
} from "../../lib/ipc";
import { api, isTauri } from "../../lib/ipc";
import { KeyCombo } from "../Keycap";
import {
  AppsIcon,
  CheckIcon,
  CloseIcon,
  DownloadIcon,
  KeyboardIcon,
  MicIcon,
  MurmurMark,
  PaletteIcon,
  PersonaIcon,
  PlusIcon,
  UploadIcon,
} from "../Icons";

type SlotKey = "voice" | "hotkeys" | "persona" | "apps" | "theme";

interface Slot {
  key: SlotKey;
  label: string;
  hint: string;
  icon: (props: { size?: number }) => ReactElement;
  summary: (loadout: Loadout) => string;
}

const VOICE_STYLES: { value: VoiceStyle; label: string; detail: string }[] = [
  { value: "crisp", label: "Crisp", detail: "Short, decisive sentences." },
  { value: "natural", label: "Natural", detail: "Your rhythm, lightly cleaned." },
  { value: "formal", label: "Formal", detail: "Professional and complete." },
  { value: "casual", label: "Casual", detail: "Relaxed and conversational." },
  { value: "custom", label: "Custom", detail: "Follow your own instructions." },
];

const PERSONA_PRESETS: { value: PersonaPreset; label: string; detail: string }[] = [
  { value: "assistant", label: "Assistant", detail: "Neutral and capable." },
  { value: "expert", label: "Expert", detail: "Precise and confident." },
  { value: "coach", label: "Coach", detail: "Warm and action-oriented." },
  { value: "friend", label: "Friend", detail: "Plain language, light warmth." },
  { value: "custom", label: "Custom", detail: "Follow your instructions." },
];

const THEME_MODES: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "System" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
];

const PALETTES: { value: ThemePalette; label: string; detail: string }[] = [
  { value: "gold", label: "Gold", detail: "Office gold on charcoal" },
  { value: "mist", label: "Mist", detail: "Cream bubbles, light-blue accent" },
];

const ACCENTS = ["#d0ad5e", "#7c8cff", "#22d3ee", "#a855f7", "#f97316", "#10b981", "#f43f5e"];

const MODIFIERS: { key: "ctrl" | "shift" | "alt" | "win"; label: string }[] = [
  { key: "ctrl", label: "Ctrl" },
  { key: "shift", label: "Shift" },
  { key: "alt", label: "Alt" },
  { key: "win", label: "Win" },
];

const GESTURE_LABEL: Record<GestureName, string> = {
  dictation: "Dictation",
  circle_to_ask: "Circle to ask",
  schedule_task: "Schedule by voice",
};

function keysFor(slot: {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
}): string[] {
  const keys: string[] = [];
  if (slot.ctrl) keys.push("Ctrl");
  if (slot.shift) keys.push("Shift");
  if (slot.alt) keys.push("Alt");
  if (slot.win) keys.push("Win");
  return keys;
}

function chordOf(slot: {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
}): string {
  return `${Number(slot.ctrl)}${Number(slot.shift)}${Number(slot.alt)}${Number(slot.win)}`;
}

function voiceGuidance(loadout: Loadout): string {
  const { style, polish, custom } = loadout.voice;
  const tone =
    style === "crisp"
      ? "Write tight, decisive sentences and prefer the shortest wording that keeps the meaning."
      : style === "formal"
        ? "Use polished, professional language and complete sentences."
        : style === "casual"
          ? "Keep the result relaxed and conversational; contractions are welcome."
          : style === "custom"
            ? "Follow the speaker's custom voice instructions below precisely."
            : "Keep the speaker's natural rhythm, contractions and tone.";
  const clean = polish
    ? "Fix punctuation, capitalisation, obvious grammar slips and filler words."
    : "Stay verbatim: keep filler words, false starts and the speaker's exact wording. Do not polish.";
  const extra = custom.trim();
  return `Voice style: ${tone} ${clean}${extra ? `\nCustom dictation instructions: ${extra}` : ""}`;
}

function personaGuidance(loadout: Loadout): string {
  const { preset, name, instructions } = loadout.persona;
  const tone =
    preset === "expert"
      ? "Speak like a senior domain expert: precise, confident and compact."
      : preset === "coach"
        ? "Be an encouraging coach: warm, direct and action-oriented."
        : preset === "friend"
          ? "Be a friendly, casual companion; use plain language and light warmth."
          : preset === "custom"
            ? "Follow the persona instructions below precisely."
            : "Be a capable, neutral desktop assistant.";
  const extra = instructions.trim();
  return `Persona: ${name.trim() || "Murmur"}. ${tone}${extra ? `\nPersona instructions: ${extra}` : ""}`;
}

const SLOTS: Slot[] = [
  {
    key: "voice",
    label: "Voice",
    hint: "How dictation is written",
    icon: MicIcon,
    summary: (loadout) => {
      const label =
        VOICE_STYLES.find((style) => style.value === loadout.voice.style)?.label ?? "Natural";
      return `${label} · ${loadout.voice.polish ? "Polish" : "Verbatim"}`;
    },
  },
  {
    key: "hotkeys",
    label: "Hotkeys",
    hint: "Keys and notch wake",
    icon: KeyboardIcon,
    summary: (loadout) => {
      const active = loadout.hotkeys.bindings.filter((slot) => slot.enabled).length;
      return `${loadout.hotkeys.hold_to_talk ? "Hold" : "Tap"} · ${active} active`;
    },
  },
  {
    key: "persona",
    label: "Persona",
    hint: "How answers sound",
    icon: PersonaIcon,
    summary: (loadout) => {
      const preset =
        PERSONA_PRESETS.find((entry) => entry.value === loadout.persona.preset)?.label ??
        "Assistant";
      return `${preset} · ${loadout.persona.name || "Murmur"}`;
    },
  },
  {
    key: "apps",
    label: "Apps",
    hint: "Pinned launches",
    icon: AppsIcon,
    summary: (loadout) => `${loadout.apps.pinned.length} pinned`,
  },
  {
    key: "theme",
    label: "Theme",
    hint: "Surface and accent",
    icon: PaletteIcon,
    summary: (loadout) => {
      const mode =
        THEME_MODES.find((entry) => entry.value === loadout.theme.mode)?.label ?? "System";
      return `${mode} · ${loadout.theme.accent.toUpperCase()}`;
    },
  },
];

interface Props {
  loadout: Loadout;
  onChange: (next: Loadout) => void;
  onNotice?: (message: string | null) => void;
}

function Chip({
  selected,
  onClick,
  children,
}: {
  selected: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      className={`rounded-lg border px-2.5 py-1.5 text-[11px] font-medium transition ${
        selected
          ? "border-accent/60 bg-accent/15 text-accent-text"
          : "border-line bg-panel-raised/50 text-zinc-300 hover:border-accent/30 hover:text-accent-text"
      }`}
    >
      {children}
    </button>
  );
}

function ToggleRow({
  label,
  detail,
  checked,
  onChange,
}: {
  label: string;
  detail: string;
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <label className="flex items-start justify-between gap-4 rounded-xl px-1 py-2.5">
      <span className="min-w-0">
        <span className="block text-[12.5px] font-medium text-zinc-200">{label}</span>
        <span className="mt-0.5 block text-[11px] leading-relaxed text-muted">{detail}</span>
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        aria-label={label}
        className={`relative mt-1 h-6 w-10 shrink-0 rounded-full border transition ${
          checked ? "border-accent/60 bg-accent/70" : "border-line bg-panel-raised"
        }`}
      >
        <span
          className={`absolute top-0.5 h-3.5 w-3.5 rounded-full bg-white shadow transition-all ${
            checked ? "left-[22px]" : "left-0.5"
          }`}
        />
      </button>
    </label>
  );
}

function PromptPreview({ title, value }: { title: string; value: string }) {
  return (
    <div className="rounded-xl border border-line/80 bg-panel-raised/40 px-3 py-2.5">
      <p className="text-[10px] font-semibold tracking-[0.12em] text-muted uppercase">{title}</p>
      <pre className="mt-1.5 whitespace-pre-wrap break-words font-sans text-[11px] leading-relaxed text-zinc-300">
        {value}
      </pre>
    </div>
  );
}

/**
 * The five-slot Loadout. The radial diagram is the map; the panel below edits
 * the selected slot. Every change is lifted to MainWindow, which owns the
 * debounced SQLite write, so this component stays a pure view of `loadout`.
 */
export function LoadoutSection({ loadout, onChange, onNotice }: Props) {
  const reducedMotion = useReducedMotion();
  const [selected, setSelected] = useState<SlotKey>("voice");
  const [newApp, setNewApp] = useState("");
  const fileInput = useRef<HTMLInputElement | null>(null);

  const patch = useCallback(
    (merge: Partial<Loadout>) => {
      onChange({ ...loadout, ...merge });
    },
    [loadout, onChange],
  );

  const duplicateChords = useMemo(() => {
    const seen = new Map<string, GestureName>();
    const duplicates = new Set<GestureName>();
    for (const slot of loadout.hotkeys.bindings) {
      if (!slot.enabled) continue;
      const chord = chordOf(slot);
      if (chord === "0000") continue;
      const previous = seen.get(chord);
      if (previous) duplicates.add(previous);
      if (previous) duplicates.add(slot.gesture);
      seen.set(chord, slot.gesture);
    }
    return duplicates;
  }, [loadout.hotkeys.bindings]);

  function patchSlot(
    gesture: GestureName,
    merge: Partial<Loadout["hotkeys"]["bindings"][number]>,
  ) {
    const bindings = loadout.hotkeys.bindings.map((slot) =>
      slot.gesture === gesture ? { ...slot, ...merge } : slot,
    );
    patch({ hotkeys: { ...loadout.hotkeys, bindings } });
  }

  function addApp() {
    const name = newApp.trim();
    if (!name) return;
    if (loadout.apps.pinned.length >= 12) {
      onNotice?.("A Loadout can pin at most 12 apps.");
      return;
    }
    if (loadout.apps.pinned.some((app) => app.toLowerCase() === name.toLowerCase())) {
      setNewApp("");
      return;
    }
    patch({ apps: { pinned: [...loadout.apps.pinned, name] } });
    setNewApp("");
  }

  async function exportJson() {
    try {
      const json = isTauri()
        ? await api.exportLoadout()
        : JSON.stringify(loadout, null, 2);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "murmur-loadout.json";
      anchor.click();
      URL.revokeObjectURL(url);
      onNotice?.(null);
    } catch (err) {
      onNotice?.(String(err));
    }
  }

  async function importFile(file: File) {
    try {
      const text = await file.text();
      const next = isTauri()
        ? await api.importLoadout(text)
        : (JSON.parse(text) as Loadout);
      onChange(next);
      onNotice?.("Loadout imported.");
    } catch (err) {
      onNotice?.(String(err));
    }
  }

  const center = 50;
  const radius = 36;
  const points = SLOTS.map((slot, index) => {
    const angle = ((-90 + index * 72) * Math.PI) / 180;
    return {
      ...slot,
      x: center + radius * Math.cos(angle),
      y: center + radius * Math.sin(angle),
    };
  });

  const active = SLOTS.find((slot) => slot.key === selected) ?? SLOTS[0]!;

  return (
    <section
      className="dashboard-surface rounded-2xl border border-line bg-panel/80 p-5"
      aria-labelledby="loadout-heading"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 id="loadout-heading" className="text-sm font-semibold text-zinc-100">
            Loadout
          </h2>
          <p className="mt-1 max-w-md text-xs leading-relaxed text-muted">
            Five slots shape how Murmur listens, answers, wakes and looks. Click a slot to edit it.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void exportJson()}
            className="flex items-center gap-1.5 rounded-lg border border-line px-2.5 py-1.5 text-[11px] font-medium text-muted transition hover:border-accent/40 hover:text-accent-text"
          >
            <DownloadIcon size={13} /> Export
          </button>
          <button
            type="button"
            onClick={() => fileInput.current?.click()}
            className="flex items-center gap-1.5 rounded-lg border border-line px-2.5 py-1.5 text-[11px] font-medium text-muted transition hover:border-accent/40 hover:text-accent-text"
          >
            <UploadIcon size={13} /> Import
          </button>
          <input
            ref={fileInput}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void importFile(file);
              event.target.value = "";
            }}
          />
        </div>
      </div>

      {/* Radial map */}
      <div className="relative mx-auto mt-6 aspect-square w-full max-w-[500px] select-none">
        <svg
          className="absolute inset-0 h-full w-full"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          {points.map((point) => {
            const lit = point.key === selected;
            return (
              <line
                key={point.key}
                x1={center}
                y1={center}
                x2={point.x}
                y2={point.y}
                stroke={lit ? "var(--color-accent)" : "var(--color-line)"}
                strokeWidth={lit ? 1.4 : 1}
                strokeDasharray={lit ? undefined : "2 2"}
                vectorEffect="non-scaling-stroke"
                opacity={lit ? 0.9 : 0.6}
              />
            );
          })}
        </svg>

        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
          <motion.div
            className="flex h-[92px] w-[92px] flex-col items-center justify-center gap-1 rounded-full border text-center"
            style={{
              borderColor: "color-mix(in oklab, var(--color-accent) 45%, transparent)",
              background:
                "radial-gradient(circle at 50% 30%, color-mix(in oklab, var(--color-accent) 26%, transparent), transparent 70%)",
              boxShadow: "0 0 30px -8px color-mix(in oklab, var(--color-accent) 60%, transparent)",
            }}
            animate={{ scale: reducedMotion ? 1 : [1, 1.02, 1] }}
            transition={reducedMotion ? { duration: 0 } : { duration: 5, repeat: Infinity, ease: "easeInOut" }}
          >
            <span style={{ color: "var(--color-accent-soft)" }}>
              <MurmurMark size={24} />
            </span>
            <span className="text-[9.5px] font-semibold tracking-[0.16em] text-muted uppercase">
              Loadout
            </span>
          </motion.div>
        </div>

        {points.map((point) => {
          const Icon = point.icon;
          const lit = point.key === selected;
          return (
            <button
              key={point.key}
              type="button"
              onClick={() => setSelected(point.key)}
              aria-pressed={lit}
              className="absolute w-[132px] -translate-x-1/2 -translate-y-1/2 rounded-2xl border px-3.5 py-3 text-left transition hover:-translate-y-[calc(50%+2px)]"
              style={{
                left: `${point.x}%`,
                top: `${point.y}%`,
                borderColor: lit
                  ? "color-mix(in oklab, var(--color-accent) 55%, transparent)"
                  : "var(--color-line)",
                background: lit
                  ? "color-mix(in oklab, var(--color-accent) 13%, var(--color-panel))"
                  : "color-mix(in oklab, var(--color-panel-raised) 70%, transparent)",
                boxShadow: lit
                  ? "0 10px 30px -16px color-mix(in oklab, var(--color-accent) 70%, transparent)"
                  : "none",
              }}
            >
              <span
                className="flex items-center gap-1.5"
                style={{ color: lit ? "var(--color-accent-soft)" : "var(--color-muted)" }}
              >
                <Icon size={14} />
                <span className="text-[11.5px] font-semibold text-zinc-200">{point.label}</span>
              </span>
              <span className="mt-1 block truncate text-[11px] text-muted">
                {point.summary(loadout)}
              </span>
            </button>
          );
        })}
      </div>

      {/* Slot editor */}
      <div className="mt-5 rounded-2xl border border-line/80 bg-panel-raised/30 p-5">
        <div className="flex items-start justify-between gap-3 border-b border-line/70 pb-4">
          <div className="flex items-center gap-2.5">
            <span
              className="flex h-8 w-8 items-center justify-center rounded-lg"
              style={{
                background: "color-mix(in oklab, var(--color-accent) 16%, transparent)",
                color: "var(--color-accent-soft)",
              }}
            >
              <active.icon size={16} />
            </span>
            <div>
              <h3 className="text-[13px] font-semibold text-zinc-100">{active.label}</h3>
              <p className="text-[11px] text-muted">{active.hint}</p>
            </div>
          </div>
          <span className="rounded-full border border-line px-2 py-0.5 text-[10px] text-muted">
            {active.summary(loadout)}
          </span>
        </div>

        <div className="pt-4">
          {selected === "voice" ? (
            <div className="space-y-4">
              <div className="flex flex-wrap gap-2">
                {VOICE_STYLES.map((style) => (
                  <Chip
                    key={style.value}
                    selected={loadout.voice.style === style.value}
                    onClick={() => patch({ voice: { ...loadout.voice, style: style.value } })}
                  >
                    {style.label}
                  </Chip>
                ))}
              </div>
              <p className="text-[11px] leading-relaxed text-muted">
                {VOICE_STYLES.find((style) => style.value === loadout.voice.style)?.detail}
              </p>
              <ToggleRow
                label="Polish the transcript"
                detail="Fix punctuation, grammar slips and filler words. Off keeps every word as spoken."
                checked={loadout.voice.polish}
                onChange={(polish) => patch({ voice: { ...loadout.voice, polish } })}
              />
              <label htmlFor="dictation-instructions" className="block text-xs font-medium text-muted">
                Extra dictation instructions
              </label>
              <textarea
                id="dictation-instructions"
                value={loadout.voice.custom}
                maxLength={600}
                rows={3}
                onChange={(event) =>
                  patch({ voice: { ...loadout.voice, custom: event.target.value } })
                }
                placeholder="Extra dictation instructions, e.g. write in British English and keep product names capitalised."
                className="w-full resize-none rounded-xl border border-line bg-panel/70 px-3 py-2 text-[12px] leading-relaxed text-zinc-200 outline-none placeholder:text-muted focus:border-accent/50"
              />
              <PromptPreview title="Added to the dictation prompt" value={voiceGuidance(loadout)} />
            </div>
          ) : null}

          {selected === "hotkeys" ? (
            <div className="space-y-4">
              <div className="divide-y divide-line/60">
                <ToggleRow
                  label="Hold to talk"
                  detail="Press and keep the chord held while you speak; releasing sends the clip. Off switches to tap-to-start, tap-to-stop."
                  checked={loadout.hotkeys.hold_to_talk}
                  onChange={(hold_to_talk) =>
                    patch({ hotkeys: { ...loadout.hotkeys, hold_to_talk } })
                  }
                />
                <ToggleRow
                  label="Click the notch"
                  detail="Hover always opens the media panel. Enable this to also open it with a click."
                  checked={loadout.hotkeys.wake_on_click}
                  onChange={(wake_on_click) =>
                    patch({ hotkeys: { ...loadout.hotkeys, wake_on_click } })
                  }
                />
                <ToggleRow
                  label="Scroll on the notch"
                  detail="Enable this to open the media panel with a wheel scroll over the notch."
                  checked={loadout.hotkeys.wake_on_scroll}
                  onChange={(wake_on_scroll) =>
                    patch({ hotkeys: { ...loadout.hotkeys, wake_on_scroll } })
                  }
                />
              </div>

              <div className="space-y-2 pt-1">
                {loadout.hotkeys.bindings.map((slot) => {
                  const duplicate = duplicateChords.has(slot.gesture);
                  const empty = keysFor(slot).length === 0;
                  const altOnly = slot.alt && !slot.ctrl && !slot.shift && !slot.win;
                  return (
                    <div
                      key={slot.gesture}
                      className="rounded-xl border px-3 py-2.5"
                      style={{
                        borderColor:
                          duplicate || altOnly
                            ? "color-mix(in oklab, #f59e0b 45%, transparent)"
                            : "var(--color-line)",
                      }}
                    >
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-[12px] font-medium text-zinc-200">
                          {GESTURE_LABEL[slot.gesture]}
                        </span>
                        <div className="flex items-center gap-2">
                          <KeyCombo keys={keysFor(slot)} />
                          <button
                            type="button"
                            role="switch"
                            aria-checked={slot.enabled}
                            aria-label={`Enable ${GESTURE_LABEL[slot.gesture]}`}
                            onClick={() =>
                              patchSlot(slot.gesture, {
                                enabled: empty || altOnly ? false : !slot.enabled,
                              })
                            }
                            disabled={empty || altOnly}
                            className={`relative h-6 w-10 shrink-0 rounded-full border transition disabled:opacity-40 ${
                              slot.enabled
                                ? "border-accent/60 bg-accent/70"
                                : "border-line bg-panel-raised"
                            }`}
                          >
                            <span
                              className={`absolute top-0.5 h-3.5 w-3.5 rounded-full bg-white shadow transition-all ${
                                slot.enabled ? "left-[22px]" : "left-0.5"
                              }`}
                            />
                          </button>
                        </div>
                      </div>
                      <div className="mt-2 flex flex-wrap gap-1.5">
                        {MODIFIERS.map((modifier) => {
                          const on = slot[modifier.key];
                          return (
                            <button
                              key={modifier.key}
                              type="button"
                              aria-pressed={on}
                              onClick={() => {
                                const next = {
                                  ctrl: slot.ctrl,
                                  shift: slot.shift,
                                  alt: slot.alt,
                                  win: slot.win,
                                  [modifier.key]: !on,
                                };
                                const merged = { ...slot, ...next };
                                const stillHasModifier =
                                  merged.ctrl || merged.shift || merged.alt || merged.win;
                                patchSlot(slot.gesture, {
                                  ...next,
                                  enabled: stillHasModifier ? slot.enabled : false,
                                });
                              }}
                              className={`rounded-md border px-2 py-1 text-[10.5px] font-medium transition ${
                                on
                                  ? "border-accent/50 bg-accent/15 text-accent-text"
                                  : "border-line text-muted hover:text-zinc-200"
                              }`}
                            >
                              {modifier.label}
                            </button>
                          );
                        })}
                      </div>
                      {duplicate ? (
                        <p className="mt-2 text-[10.5px] leading-relaxed text-amber-300">
                          Another gesture uses this chord. It will be switched off when saved.
                        </p>
                      ) : null}
                      {empty ? (
                        <p className="mt-2 text-[10.5px] leading-relaxed text-muted">
                          Pick at least one modifier to use this gesture.
                        </p>
                      ) : null}
                      {altOnly ? (
                        <p className="mt-2 text-[10.5px] leading-relaxed text-amber-300">
                          Alt on its own stays off: holding it would fire while you use Alt+Tab.
                          Add Ctrl, Shift or Win.
                        </p>
                      ) : null}
                    </div>
                  );
                })}
              </div>
              <p className="text-[10.5px] leading-relaxed text-muted">
                Changes apply to the keyboard hook immediately and are stored in the Loadout.
              </p>
            </div>
          ) : null}

          {selected === "persona" ? (
            <div className="space-y-4">
              <div className="flex flex-wrap gap-2">
                {PERSONA_PRESETS.map((preset) => (
                  <Chip
                    key={preset.value}
                    selected={loadout.persona.preset === preset.value}
                    onClick={() =>
                      patch({ persona: { ...loadout.persona, preset: preset.value } })
                    }
                  >
                    {preset.label}
                  </Chip>
                ))}
              </div>
              <p className="text-[11px] leading-relaxed text-muted">
                {PERSONA_PRESETS.find((preset) => preset.value === loadout.persona.preset)?.detail}
              </p>
              <label className="block">
                <span className="mb-1 block text-[11px] font-medium text-muted">Name</span>
                <input
                  value={loadout.persona.name}
                  maxLength={60}
                  onChange={(event) =>
                    patch({ persona: { ...loadout.persona, name: event.target.value } })
                  }
                  className="w-full rounded-xl border border-line bg-panel/70 px-3 py-2 text-[12px] text-zinc-200 outline-none focus:border-accent/50"
                />
              </label>
              <textarea
                value={loadout.persona.instructions}
                maxLength={1500}
                rows={3}
                onChange={(event) =>
                  patch({ persona: { ...loadout.persona, instructions: event.target.value } })
                }
                placeholder="Extra answer instructions, e.g. lead with the answer and stay under 80 words."
                className="w-full resize-none rounded-xl border border-line bg-panel/70 px-3 py-2 text-[12px] leading-relaxed text-zinc-200 outline-none placeholder:text-muted focus:border-accent/50"
              />
              <PromptPreview title="Added to the answer prompt" value={personaGuidance(loadout)} />
            </div>
          ) : null}

          {selected === "apps" ? (
            <div className="space-y-4">
              <p className="text-[11px] leading-relaxed text-muted">
                Pinned apps appear in the Control Centre and open by name. Up to 12.
              </p>
              <div className="flex flex-wrap gap-2">
                {loadout.apps.pinned.map((app) => (
                  <span
                    key={app}
                    className="flex items-center gap-1.5 rounded-lg border border-line bg-panel/60 py-1 pl-2.5 pr-1.5 text-[11px] text-zinc-200"
                  >
                    {app}
                    <button
                      type="button"
                      aria-label={`Unpin ${app}`}
                      onClick={() =>
                        patch({
                          apps: {
                            pinned: loadout.apps.pinned.filter((entry) => entry !== app),
                          },
                        })
                      }
                      className="rounded p-0.5 text-muted transition hover:text-zinc-100"
                    >
                      <CloseIcon size={11} />
                    </button>
                  </span>
                ))}
                {loadout.apps.pinned.length === 0 ? (
                  <span className="text-[11px] text-muted">Nothing pinned yet.</span>
                ) : null}
              </div>
              <div className="flex gap-2">
                <input
                  value={newApp}
                  onChange={(event) => setNewApp(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      addApp();
                    }
                  }}
                  maxLength={80}
                  placeholder="App name, e.g. Spotify"
                  className="min-w-0 flex-1 rounded-xl border border-line bg-panel/70 px-3 py-2 text-[12px] text-zinc-200 outline-none placeholder:text-muted focus:border-accent/50"
                />
                <button
                  type="button"
                  onClick={addApp}
                  disabled={!newApp.trim() || loadout.apps.pinned.length >= 12}
                  className="flex items-center gap-1 rounded-xl border border-line px-3 py-2 text-[11px] font-medium text-zinc-300 transition hover:border-accent/40 hover:text-accent-text disabled:opacity-40"
                >
                  <PlusIcon size={13} /> Pin
                </button>
              </div>
              <p className="text-[10.5px] leading-relaxed text-muted">
                Names are resolved through App Paths and the Start Menu when launched.
              </p>
            </div>
          ) : null}

          {selected === "theme" ? (
            <div className="space-y-4">
              <div className="flex flex-wrap gap-2">
                {THEME_MODES.map((mode) => (
                  <Chip
                    key={mode.value}
                    selected={loadout.theme.mode === mode.value}
                    onClick={() => patch({ theme: { ...loadout.theme, mode: mode.value } })}
                  >
                    {mode.label}
                  </Chip>
                ))}
              </div>
              <div className="flex flex-wrap gap-2">
                {PALETTES.map((palette) => (
                  <Chip
                    key={palette.value}
                    selected={loadout.theme.palette === palette.value}
                    onClick={() => patch({ theme: { ...loadout.theme, palette: palette.value } })}
                  >
                    {palette.label}
                  </Chip>
                ))}
                <span className="self-center text-[11px] text-muted">
                  {PALETTES.find((entry) => entry.value === loadout.theme.palette)?.detail}
                </span>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {ACCENTS.map((accent) => (
                  <button
                    key={accent}
                    type="button"
                    aria-label={`Use accent ${accent}`}
                    aria-pressed={loadout.theme.accent.toLowerCase() === accent}
                    onClick={() => patch({ theme: { ...loadout.theme, accent } })}
                    className="flex h-7 w-7 items-center justify-center rounded-full border transition hover:scale-105"
                    style={{
                      background: accent,
                      borderColor:
                        loadout.theme.accent.toLowerCase() === accent
                          ? "rgba(255,255,255,0.9)"
                          : "transparent",
                      boxShadow:
                        loadout.theme.accent.toLowerCase() === accent
                          ? `0 0 0 2px color-mix(in oklab, ${accent} 45%, transparent)`
                          : "none",
                    }}
                  >
                    {loadout.theme.accent.toLowerCase() === accent ? (
                      <CheckIcon size={13} className="text-white" />
                    ) : null}
                  </button>
                ))}
                <label className="ml-1 flex items-center gap-2 text-[11px] text-muted">
                  Custom
                  <input
                    type="color"
                    value={loadout.theme.accent}
                    onChange={(event) =>
                      patch({ theme: { ...loadout.theme, accent: event.target.value } })
                    }
                    className="h-7 w-10 cursor-pointer rounded border border-line bg-transparent"
                  />
                </label>
              </div>
              <div
                className="rounded-xl border border-line/80 px-3 py-3"
                style={{
                  background:
                    "linear-gradient(120deg, color-mix(in oklab, var(--color-accent) 18%, transparent), transparent 65%)",
                }}
              >
                <p className="text-[12px] font-medium text-zinc-200">Accent preview</p>
                <p className="mt-1 text-[11px] leading-relaxed text-muted">
                  The accent colours the Loadout map, dashboard and notch focus rings. The Control Centre keeps its dark and gold palette. Mode{" "}
                  <span className="text-accent-text">{loadout.theme.mode}</span> applies instantly.
                </p>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}
