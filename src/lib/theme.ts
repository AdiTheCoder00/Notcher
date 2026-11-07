import type { Loadout, ThemeSettings } from "./ipc";

export function defaultLoadout(): Loadout {
  return {
    version: 1,
    voice: { style: "natural", polish: true, custom: "" },
    hotkeys: {
      hold_to_talk: true,
      wake_on_click: true,
      wake_on_scroll: true,
      bindings: [
        { gesture: "dictation", ctrl: true, shift: false, alt: false, win: true, enabled: true },
        {
          gesture: "circle_to_ask",
          ctrl: true,
          shift: true,
          alt: false,
          win: false,
          enabled: true,
        },
        {
          gesture: "schedule_task",
          ctrl: true,
          shift: false,
          alt: true,
          win: false,
          enabled: true,
        },
      ],
    },
    persona: { preset: "assistant", name: "Murmur", instructions: "" },
    apps: { pinned: ["Spotify", "Notepad", "Settings"] },
    theme: { mode: "system", accent: "#7c8cff" },
  };
}

let stopListening: (() => void) | undefined;

/** Applies the Loadout theme to the document and keeps `system` mode live. */
export function applyTheme(theme: ThemeSettings): void {
  stopListening?.();
  stopListening = undefined;

  const root = document.documentElement;
  const media = window.matchMedia("(prefers-color-scheme: light)");

  const paint = () => {
    const dark = theme.mode === "dark" || (theme.mode === "system" && !media.matches);
    root.dataset.theme = dark ? "dark" : "light";
    root.style.colorScheme = dark ? "dark" : "light";
  };

  root.style.setProperty("--color-accent", theme.accent);
  root.style.setProperty(
    "--color-accent-soft",
    `color-mix(in oklab, ${theme.accent} 68%, white)`,
  );
  root.style.setProperty("--notch-accent", theme.accent);
  paint();

  if (theme.mode === "system") {
    const listener = () => paint();
    media.addEventListener("change", listener);
    stopListening = () => media.removeEventListener("change", listener);
  }
}
