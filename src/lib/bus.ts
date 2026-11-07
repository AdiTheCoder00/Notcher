import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import {
  isTauri,
  type AudioLevel,
  type HotkeySnapshot,
  type Loadout,
  type NotchPayload,
  type ScheduledTask,
} from "./ipc";

/**
 * The Rust → frontend event bus. Every name and payload lives here: Rust owns
 * `notch://state`, `audio://level`, `hotkeys://state` and `task://due`, and a
 * rename on either side breaks the types instead of silently doing nothing.
 */
export const EVENTS = {
  notchState: "notch://state",
  audioLevel: "audio://level",
  hotkeyState: "hotkeys://state",
  taskDue: "task://due",
  controlState: "control://state",
  loadoutFocus: "app://loadout-focus",
  loadoutChanged: "loadout://changed",
} as const;

export type EventName = (typeof EVENTS)[keyof typeof EVENTS];

export interface EventPayloads {
  "notch://state": NotchPayload;
  "audio://level": AudioLevel;
  "hotkeys://state": HotkeySnapshot;
  "task://due": ScheduledTask;
  "control://state": { open: boolean };
  "app://loadout-focus": null;
  "loadout://changed": Loadout;
}

export type EventHandler<K extends EventName> = (payload: EventPayloads[K]) => void;

/** Subscribes to one event. Resolves to the unlisten function. */
export function on<K extends EventName>(name: K, handler: EventHandler<K>): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(name, (event) => handler(event.payload));
}

/**
 * React binding for the bus: subscribes on mount, re-subscribes only if the
 * event name changes, and always calls the latest handler so closures do not
 * go stale. No-op in a plain browser, where there is no Rust side.
 */
export function useRustEvent<K extends EventName>(name: K, handler: EventHandler<K>): void {
  const latest = useRef(handler);

  useEffect(() => {
    latest.current = handler;
  }, [handler]);

  useEffect(() => {
    if (!isTauri()) return;

    let stop: UnlistenFn | undefined;
    let cancelled = false;

    on(name, (payload) => latest.current(payload))
      .then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      stop?.();
    };
  }, [name]);
}
