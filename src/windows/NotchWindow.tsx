import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, MotionConfig } from "framer-motion";
import { EVENTS, useRustEvent } from "../lib/bus";
import { api, isTauri, type NotchPayload } from "../lib/ipc";
import { NotchBar } from "../components/notch/NotchBar";
import { ControlCentre } from "../components/control/ControlCentre";
import { applyTheme, defaultLoadout } from "../lib/theme";

const IDLE: NotchPayload = { state: "idle", text: null };

/**
 * The notch overlay. Rust owns the window (placement, click-through, show) and
 * pushes `notch://state`; this view draws it, and grows the same window into
 * the Control Centre when the resting mark is hovered, clicked, or scrolled.
 */
export default function NotchWindow() {
  const [payload, setPayload] = useState<NotchPayload>(IDLE);
  const [controlOpen, setControlOpen] = useState(false);
  const [loadout, setLoadout] = useState(defaultLoadout);

  useRustEvent(EVENTS.notchState, setPayload);
  useRustEvent(EVENTS.controlState, ({ open }) => setControlOpen(open));
  // A save in the settings window restyles the notch and can change whether
  // its resting mark reacts to clicks or the wheel.
  useRustEvent(EVENTS.loadoutChanged, setLoadout);

  // The notch is a second window, so it has to load the same Loadout to colour
  // itself and to know whether the resting mark is clickable.
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    api
      .getLoadout()
      .then((saved) => {
        if (!cancelled) setLoadout(saved);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    applyTheme(loadout.theme);
  }, [loadout.theme]);

  const dismiss = useCallback(() => {
    setPayload(IDLE);
    if (isTauri()) {
      api.dismissNotch().catch(() => undefined);
    }
  }, []);

  const wake = useCallback(
    (mode: "click" | "scroll" | "hover") => {
      if (controlOpen || payload.state !== "idle") return;

      if (mode === "hover") {
        setControlOpen(true);
        if (isTauri()) {
          api.openControlCentre().catch(() => setControlOpen(false));
        }
        return;
      }

      const allowed =
        mode === "click" ? loadout.hotkeys.wake_on_click : loadout.hotkeys.wake_on_scroll;
      if (!allowed) return;

      // Show the panel immediately; Rust resizes the window and echoes the
      // same state on `control://state`.
      setControlOpen(true);
      if (isTauri()) {
        api.openControlCentre().catch(() => setControlOpen(false));
      }
    },
    [controlOpen, loadout.hotkeys.wake_on_click, loadout.hotkeys.wake_on_scroll, payload.state],
  );

  const close = useCallback(() => {
    setControlOpen(false);
    if (isTauri()) {
      api.closeControlCentre().catch(() => undefined);
    }
  }, []);

  // Keep the plain-browser preview idle so its hover target can open the
  // Control Centre. Individual notch states remain available in the dashboard.
  useEffect(() => {
    if (isTauri()) return;
    setPayload(IDLE);
  }, []);

  return (
    <MotionConfig reducedMotion="user">
      <div
        className="relative flex h-full w-full select-none justify-center"
        onWheel={() => {
          if (controlOpen) return;
          wake("scroll");
        }}
      >
        <AnimatePresence mode="wait" initial={false}>
          {!controlOpen ? (
            <div
              key="notch"
              role="status"
              aria-live="polite"
              onMouseEnter={() => wake("hover")}
              onClick={() => wake("click")}
              onMouseLeave={() => {
                if (controlOpen) close();
              }}
              className={
                payload.state === "idle" ? "cursor-pointer" : undefined
              }
            >
              <NotchBar state={payload.state} text={payload.text} onDismiss={dismiss} />
            </div>
          ) : null}
        </AnimatePresence>
        <div
          className={`notch-control-layer${controlOpen ? " is-open" : ""}`}
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 1,
            visibility: controlOpen ? "visible" : "hidden",
            opacity: controlOpen ? 1 : 0,
            pointerEvents: controlOpen ? "auto" : "none",
            transition: "opacity 170ms ease, visibility 170ms ease",
          }}
          aria-hidden={!controlOpen}
          inert={!controlOpen}
        >
          <ControlCentre onClose={close} active={controlOpen} />
        </div>
      </div>
    </MotionConfig>
  );
}
