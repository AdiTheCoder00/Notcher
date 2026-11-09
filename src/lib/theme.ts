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
    theme: { mode: "system", accent: "#d0ad5e", palette: "gold" },
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
    // Soft accent is used as ink in the Loadout map: pale reads on the dark
    // canvas, but on white it needs to mix toward the palette foreground.
    root.style.setProperty(
      "--color-accent-soft",
      dark
        ? `color-mix(in oklab, ${theme.accent} 68%, white)`
        : `color-mix(in oklab, ${theme.accent} 55%, #0f172a)`,
    );
  };

  root.style.setProperty("--color-accent", theme.accent);
  const luminance = ([[1, 0.2126], [3, 0.7152], [5, 0.0722]] as const).reduce((sum, [offset, weight]) => {
    const channel = parseInt(theme.accent.slice(offset, offset + 2), 16) / 255;
    return sum + weight * (channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  }, 0);
  root.style.setProperty("--color-on-accent", luminance > 0.179 ? "#000000" : "#ffffff");
  root.style.setProperty("--notch-accent", theme.accent);
  root.dataset.palette = theme.palette === "mist" ? "mist" : "gold";
  paint();

  if (theme.mode === "system") {
    const listener = () => paint();
    media.addEventListener("change", listener);
    stopListening = () => media.removeEventListener("change", listener);
  }
}
