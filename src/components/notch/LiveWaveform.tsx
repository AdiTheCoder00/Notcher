import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { EVENTS, useRustEvent } from "../../lib/bus";
import { isTauri } from "../../lib/ipc";

const BARS = 24;

function emptyLevels(): number[] {
  return new Array<number>(BARS).fill(0);
}

/**
 * Live microphone history. Rust emits `audio://level` at 20 Hz with the peak of
 * the last block; each event shifts the history one bar left, so the waveform
 * scrolls instead of pulsing as a single block.
 */
export function LiveWaveform({ active }: { active: boolean }) {
  const [levels, setLevels] = useState<number[]>(emptyLevels);

  useRustEvent(EVENTS.audioLevel, (payload) => {
    setLevels((previous) => {
      const next = previous.slice(1);
      next.push(payload.recording ? Math.min(1, Math.max(0, payload.level)) : 0);
      return next;
    });
  });

  // Design preview in a plain browser: there is no meter to read there.
  useEffect(() => {
    if (isTauri() || !active) return;
    const timer = window.setInterval(() => {
      setLevels((previous) => {
        const next = previous.slice(1);
        next.push(0.12 + Math.random() * 0.72);
        return next;
      });
    }, 70);
    return () => window.clearInterval(timer);
  }, [active]);

  return (
    <span className="flex h-7 shrink-0 items-center gap-[3px]" aria-hidden="true">
      {levels.map((level, index) => (
        <motion.span
          key={index}
          className="w-[3px] rounded-full"
          style={{
            height: 26,
            transformOrigin: "center",
            background: "var(--notch-accent)",
          }}
          animate={{
            scaleY: active ? 0.08 + level * 0.92 : 0.08,
            opacity: active ? 1 : 0.35,
          }}
          transition={{ type: "spring", stiffness: 320, damping: 26, mass: 0.5 }}
        />
      ))}
    </span>
  );
}
