import { useEffect, useRef, useState } from "react";

interface Options {
  durationMs: number;
  paused: boolean;
  onDone: () => void;
}

/**
 * Countdown that survives pauses: hover freezes it where it is and releasing
 * continues from the same point. Returns the remaining fraction (1 → 0) for the
 * progress line under the card.
 */
export function useDismissTimer({ durationMs, paused, onDone }: Options): number {
  const [remainingMs, setRemainingMs] = useState(durationMs);
  const done = useRef(onDone);

  useEffect(() => {
    done.current = onDone;
  }, [onDone]);

  useEffect(() => {
    if (paused) return;

    const startedAt = Date.now();
    const budget = remainingMs;

    const timer = window.setInterval(() => {
      const left = budget - (Date.now() - startedAt);
      setRemainingMs(Math.max(0, left));
      if (left <= 0) {
        window.clearInterval(timer);
        done.current();
      }
    }, 100);

    return () => window.clearInterval(timer);
    // `remainingMs` is the budget captured when this run starts; depending on it
    // would restart the clock on every tick.
  }, [paused, durationMs]);

  return durationMs > 0 ? remainingMs / durationMs : 0;
}
