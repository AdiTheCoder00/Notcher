import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import type { NotchStateName } from "../../lib/ipc";
import { AnswerCard } from "./AnswerCard";
import { ErrorPill } from "./ErrorPill";
import { IdleMark } from "./IdleMark";
import { LiveWaveform } from "./LiveWaveform";
import { ThinkingDots } from "./ThinkingDots";
import { SPRING } from "./springs";

const RADIUS: Record<NotchStateName, number> = {
  idle: 999,
  listening: 999,
  thinking: 999,
  answer: 20,
  error: 22,
};

interface Props {
  state: NotchStateName;
  text: string | null;
  onDismiss: () => void;
}

function Body({ state, text, onDismiss }: Props) {
  switch (state) {
    case "idle":
      return (
        <div className="flex h-[26px] w-[168px] items-center justify-center">
          <IdleMark />
        </div>
      );

    case "listening":
      return (
        <div className="flex h-14 w-[340px] max-w-[calc(100vw-24px)] items-center gap-3 pl-4 pr-3.5">
          <LiveWaveform active />
          <span
            className="min-w-0 flex-1 truncate text-[13px] font-medium"
            style={{ color: "var(--notch-text)" }}
          >
            {text ?? "Listening…"}
          </span>
          <motion.span
            className="h-1.5 w-1.5 shrink-0 rounded-full"
            style={{ background: "var(--notch-danger)" }}
            animate={{ opacity: [0.35, 1, 0.35] }}
            transition={{ duration: 1.6, repeat: Infinity, ease: "easeInOut" }}
            aria-hidden="true"
          />
        </div>
      );

    case "thinking":
      return (
        <div className="flex h-12 w-[228px] items-center gap-3 px-4">
          <ThinkingDots />
          <span
            className="min-w-0 flex-1 truncate text-[12.5px] font-medium"
            style={{ color: "var(--notch-text)" }}
          >
            {text ?? "Thinking…"}
          </span>
        </div>
      );

    case "answer":
      return <AnswerCard markdown={text ?? ""} onDismiss={onDismiss} />;

    case "error":
      return <ErrorPill message={text ?? "Something went wrong"} />;

    default:
      return null;
  }
}

/**
 * The pill. Width and height are content-driven: `layout` measures whichever
 * body is present and springs the container to it, so every state change is a
 * physical resize rather than a hard swap.
 */
export function NotchBar({ state, text, onDismiss }: Props) {
  const reduced = useReducedMotion();
  const transition = reduced ? { duration: 0 } : SPRING;

  return (
    <motion.div
      layout={!reduced}
      initial={false}
      animate={{
        borderRadius: state === "idle" ? "0 0 22px 22px" : RADIUS[state],
      }}
      transition={transition}
      className="relative overflow-hidden border"
      style={{
        background:
          state === "idle"
            ? "rgba(4, 5, 7, 0.98)"
            : state === "error"
              ? "var(--notch-danger-surface)"
              : "var(--notch-surface)",
        borderColor:
          state === "idle"
            ? "transparent"
            : state === "error"
              ? "color-mix(in oklab, var(--notch-danger) 38%, transparent)"
              : "var(--notch-border)",
        boxShadow: state === "idle" ? "0 5px 16px rgba(0,0,0,0.2)" : "var(--notch-shadow)",
        backdropFilter: "blur(30px) saturate(150%)",
        WebkitBackdropFilter: "blur(30px) saturate(150%)",
      }}
    >
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={state}
          initial={{ opacity: 0, y: reduced ? 0 : -3 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: reduced ? 0 : -2 }}
          transition={{ duration: reduced ? 0 : 0.15, ease: "easeOut" }}
          className="flex"
        >
          <Body state={state} text={text} onDismiss={onDismiss} />
        </motion.div>
      </AnimatePresence>
    </motion.div>
  );
}
