import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import type { NotchStateName } from "../../lib/ipc";
import { AnswerCard } from "./AnswerCard";
import { DictationBubble } from "./DictationBubble";
import { ErrorPill } from "./ErrorPill";
import { IdleMark } from "./IdleMark";
import { isPillAnswer } from "./pillAnswer";
import { SPRING } from "./springs";
import { ThinkingDots } from "./ThinkingDots";

const RADIUS: Record<NotchStateName, number> = {
  idle: 999,
  listening: 18,
  thinking: 999,
  answer: 20,
  error: 22,
};

const FALLBACK_KEYS = ["Ctrl", "Win"];

interface Props {
  state: NotchStateName;
  text: string | null;
  keys?: string[];
  onDismiss: () => void;
}

function Body({ state, text, keys, onDismiss }: Props) {
  switch (state) {
    case "idle":
      return (
        <div className="flex h-[26px] w-[168px] items-center justify-center">
          <IdleMark />
        </div>
      );

    case "listening":
      return (
        <DictationBubble keys={keys ?? FALLBACK_KEYS} text={text ?? "Listening…"} />
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
 * physical resize rather than a hard swap. The dictation bubble draws its own
 * surfaces, so the container stays transparent for that state.
 */
export function NotchBar({ state, text, keys, onDismiss }: Props) {
  const reduced = useReducedMotion();
  const transition = reduced ? { duration: 0 } : SPRING;
  const bubble = state === "listening";
  const pill = state === "answer" && isPillAnswer(text);

  return (
    <motion.div
      layout={!reduced}
      initial={false}
      animate={{
        borderRadius:
          state === "idle" ? "0 0 22px 22px" : pill ? 999 : RADIUS[state],
      }}
      transition={transition}
      className={`notch-pill notch-pill--${state} relative border`}
      style={{
        overflow: bubble ? "visible" : "hidden",
        background: bubble
          ? "transparent"
          : state === "idle"
            ? "rgba(4, 5, 7, 0.98)"
            : state === "error"
              ? "var(--notch-danger-surface)"
              : "var(--notch-surface)",
        borderColor: bubble
          ? "transparent"
          : state === "idle"
            ? "transparent"
            : state === "error"
              ? "color-mix(in oklab, var(--notch-danger) 38%, transparent)"
              : "var(--notch-border)",
        boxShadow:
          state === "idle"
            ? "0 5px 16px rgba(0,0,0,0.2)"
            : bubble
              ? "none"
              : "var(--notch-shadow)",
        backdropFilter: bubble ? undefined : "blur(30px) saturate(150%)",
        WebkitBackdropFilter: bubble ? undefined : "blur(30px) saturate(150%)",
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
          <Body state={state} text={text} keys={keys} onDismiss={onDismiss} />
        </motion.div>
      </AnimatePresence>
    </motion.div>
  );
}
