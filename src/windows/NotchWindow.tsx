import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, MotionConfig, motion, useReducedMotion } from "framer-motion";
import { EVENTS, useRustEvent } from "../lib/bus";
import { api, isTauri, type NotchPayload } from "../lib/ipc";
import { NotchBar } from "../components/notch/NotchBar";
import { ControlCentre } from "../components/control/ControlCentre";
import { applyTheme, defaultLoadout } from "../lib/theme";

const IDLE: NotchPayload = { state: "idle", text: null };

// Browser-only affordances for scripts/check-notch-scales.mjs: `?control=open`
// renders the panel where nothing can hover the resting mark, and `?probe=1`
// drops the drop shadow so the carved corners sit against a clean canvas.
const query = new URLSearchParams(window.location.search);
const AUTO_OPEN = !isTauri() && query.get("control") === "open";
if (!isTauri() && query.get("probe") === "1") {
  document.documentElement.dataset.probe = "1";
}

/**
 * The notch overlay. Rust owns the window (placement, click-through, show) and
 * pushes `notch://state`; this view draws it. Hovering the resting mark grows a
 * compact media card, and clicking that card — anywhere except its controls —
 * opens the full Control Centre.
 */
export default function NotchWindow() {
  const [payload, setPayload] = useState<NotchPayload>(IDLE);
  const [controlOpen, setControlOpen] = useState(AUTO_OPEN);
  const [peekOpen, setPeekOpen] = useState(false);
  // Kept mounted through the collapse so the panel's contents animate out
  // instead of vanishing the moment `controlOpen` flips false.
  const [contentMounted, setContentMounted] = useState(AUTO_OPEN);
  const [focusRequested, setFocusRequested] = useState(false);
  const [loadout, setLoadout] = useState(defaultLoadout);
  const openedAt = useRef(0);
  const closing = useRef(false);
  const peekOpenRef = useRef(false);
  const peekSkipClose = useRef(false);
  const reducedMotion = useReducedMotion();
  const open = controlOpen || peekOpen;
  const Trigger = payload.state === "idle" ? "button" : "div";
  const keyboardOpen = useRef(false);

  // AnimatePresence reads this after the exit animation, when the state
  // captured by the handler is already stale.
  useEffect(() => {
    peekOpenRef.current = peekOpen;
  }, [peekOpen]);

  useRustEvent(EVENTS.notchState, setPayload);
  useRustEvent(EVENTS.controlFocus, () => setFocusRequested(true));
  useRustEvent(EVENTS.controlState, ({ open: isOpen, mode }) => {
    if (!isOpen) {
      closing.current = false;
      peekSkipClose.current = true;
      setControlOpen(false);
      setContentMounted(false);
      setPeekOpen(false);
      setFocusRequested(false);
      return;
    }
    // Rust resized the window; mirror which card it is showing.
    setContentMounted(mode !== "peek");
    setControlOpen(mode !== "peek");
    setPeekOpen(mode === "peek");
  });
  // A save in the settings window restyles the notch and can change whether
  // its resting mark reacts to clicks or the wheel.
  useRustEvent(EVENTS.loadoutChanged, setLoadout);
  // The dictation bubble advertises whichever chord actually starts dictation.
  const dictationKeys = (() => {
    const slot = loadout.hotkeys.bindings.find(
      (entry) => entry.gesture === "dictation" && entry.enabled,
    );
    if (!slot) return ["Ctrl", "Win"];
    return [
      slot.ctrl ? "Ctrl" : null,
      slot.shift ? "Shift" : null,
      slot.alt ? "Alt" : null,
      slot.win ? "Win" : null,
    ].filter((key): key is string => key !== null);
  })();

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

  useEffect(() => {
    if (!controlOpen || !focusRequested) return;
    document.querySelector<HTMLButtonElement>(".cc-overlay .cc-tab[aria-current]")?.focus({ preventScroll: true });
    setFocusRequested(false);
  }, [controlOpen, focusRequested]);

  const dismiss = useCallback(() => {
    setPayload(IDLE);
    if (isTauri()) {
      api.dismissNotch().catch(() => undefined);
    }
  }, []);

  /** Grows the full Control Centre, replacing the media card if one is open. */
  const openFull = useCallback((focus = false) => {
    if (closing.current) return;
    keyboardOpen.current = focus;
    setFocusRequested(focus);
    openedAt.current = focus ? 0 : Date.now();
    peekSkipClose.current = true;
    setPeekOpen(false);
    setContentMounted(true);
    setControlOpen(true);
    if (isTauri()) {
      (focus ? api.openLauncher() : api.openControlCentre()).catch(() => setControlOpen(false));
    }
  }, []);

  const closePeek = useCallback(() => {
    setPeekOpen(false);
  }, []);

  const wake = useCallback(
    (mode: "click" | "scroll" | "hover") => {
      // Rust must release the full panel's size before a hover can reopen it.
      if (open || closing.current || payload.state !== "idle") return;

      // Hover is only a preview: the media card, not the whole panel.
      if (mode === "hover") {
        openedAt.current = Date.now();
        peekSkipClose.current = false;
        setPeekOpen(true);
        if (isTauri()) {
          api.openNotchPeek().catch(() => setPeekOpen(false));
        }
        return;
      }

      const allowed =
        mode === "click" ? loadout.hotkeys.wake_on_click : loadout.hotkeys.wake_on_scroll;
      if (!allowed) return;

      // A click or wheel on the resting mark asks for the full Control Centre.
      openFull();
    },
    [open, loadout.hotkeys.wake_on_click, loadout.hotkeys.wake_on_scroll, payload.state, openFull],
  );

  const close = useCallback(() => {
    closing.current = true;
    setControlOpen(false);
    setPeekOpen(false);
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
        onMouseLeave={() => { if (peekOpen) closePeek(); }}
        onWheel={() => {
          if (open) return;
          wake("scroll");
        }}
      >
        <AnimatePresence mode="wait" initial={false}>
          {!open ? (
            <Trigger
              key="notch"
              type={payload.state === "idle" ? "button" : undefined}
              role={payload.state === "idle" ? undefined : "status"}
              aria-label={payload.state === "idle" ? "Open Control Centre" : undefined}
              aria-live={payload.state === "idle" ? undefined : "polite"}
              onMouseEnter={() => wake("hover")}
              onClick={(event) => {
                if (payload.state === "idle" && event.detail === 0) openFull(true);
                else wake("click");
              }}
              onMouseLeave={() => {
                if (open) close();
              }}
              className={
                payload.state === "idle" ? "notch-idle-shell cursor-pointer" : undefined
              }
            >
              <NotchBar
                state={payload.state}
                text={payload.text}
                keys={dictationKeys}
                onDismiss={dismiss}
              />
            </Trigger>
          ) : null}
        </AnimatePresence>
        <motion.div
          className={`notch-control-layer${open ? " is-open" : ""}`}
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 1,
            pointerEvents: open ? "auto" : "none",
            transformOrigin: "top center",
          }}
          initial={false}
          animate={
            open
              ? { opacity: 1, scaleX: 1, scaleY: 1 }
              : { opacity: 0, scaleX: reducedMotion ? 1 : 0.22, scaleY: reducedMotion ? 1 : 0.07 }
          }
          transition={
            reducedMotion
              ? { duration: 0.12 }
              : { type: "spring", stiffness: 380, damping: 34, mass: 0.8 }
          }
          onAnimationComplete={() => {
            if (!open && closing.current) {
              setContentMounted(false);
              const finishClose = () => {
                closing.current = false;
                const trigger = document.querySelector<HTMLButtonElement>(".notch-idle-shell");
                if (keyboardOpen.current) {
                  keyboardOpen.current = false;
                  trigger?.focus();
                } else if (trigger?.matches(":hover")) {
                  wake("hover");
                }
              };
              if (isTauri()) api.closeControlCentre().then(finishClose).catch(() => { closing.current = false; });
              else finishClose();
            }
          }}
          aria-hidden={!open}
          inert={!open}
          onClickCapture={(event) => {
            // The click that wakes the growing notch can otherwise land on a tab.
            if (controlOpen && Date.now() - openedAt.current < 350) {
              event.preventDefault();
              event.stopPropagation();
            }
          }}
        >
          <div className="h-full w-full" hidden={!contentMounted}>
            <ControlCentre onClose={close} active={controlOpen} />
          </div>
          <AnimatePresence
            onExitComplete={() => {
              // Rust owns the peek window size, so the resize waits for the
              // card to finish leaving. A reopen or a promotion to the full
              // panel makes this a no-op.
              if (peekOpenRef.current || peekSkipClose.current) return;
              if (isTauri()) api.closeNotchPeek().catch(() => undefined);
            }}
          >
            {peekOpen ? (
              <motion.div
                key="peek"
                className="h-full w-full"
                initial={false}
                exit={{
                  opacity: 0,
                  y: reducedMotion ? 0 : -6,
                  scale: reducedMotion ? 1 : 0.985,
                }}
                transition={
                  reducedMotion ? { duration: 0.12 } : { duration: 0.18, ease: "easeOut" }
                }
                onClickCapture={(event) => {
                  // The card's own buttons keep their behaviour; anything else
                  // is a request for the full panel.
                  const target = event.target as HTMLElement | null;
                  if (target?.closest("button, a, input, [role='button']")) return;
                  event.preventDefault();
                  event.stopPropagation();
                  openFull();
                }}
              >
                <ControlCentre peek active onClose={closePeek} />
              </motion.div>
            ) : null}
          </AnimatePresence>
        </motion.div>
      </div>
    </MotionConfig>
  );
}
