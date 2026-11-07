import { useCallback, useState } from "react";
import { motion } from "framer-motion";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, isTauri } from "../../lib/ipc";
import { useDismissTimer } from "./useDismissTimer";

const AUTO_DISMISS_MS = 20_000;

interface Props {
  markdown: string;
  onDismiss: () => void;
}

/**
 * The answer state: a card that expands from the pill, renders the model's
 * markdown, offers a copy button, and dismisses itself after 20 s unless the
 * pointer is inside it.
 */
export function AnswerCard({ markdown, onDismiss }: Props) {
  const [paused, setPaused] = useState(false);
  const [copied, setCopied] = useState(false);
  const progress = useDismissTimer({ durationMs: AUTO_DISMISS_MS, paused, onDone: onDismiss });

  const copy = useCallback(async () => {
    try {
      if (isTauri()) {
        await api.copyText(markdown);
      } else {
        await navigator.clipboard.writeText(markdown);
      }
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // Clipboard access can be refused; the card stays usable either way.
    }
  }, [markdown]);

  return (
    <motion.div
      className="flex w-[468px] max-w-[calc(100vw-24px)] flex-col"
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.14 }}
    >
      <div className="flex items-start gap-3 px-4 pt-3.5">
        <div className="notch-md min-w-0 flex-1 overflow-y-auto" style={{ maxHeight: 176 }}>
          <Markdown remarkPlugins={[remarkGfm]}>{markdown}</Markdown>
        </div>

        <button
          type="button"
          onClick={() => void copy()}
          className="shrink-0 rounded-md border px-2 py-1 text-[11px] font-medium transition hover:opacity-80"
          style={{
            borderColor: "var(--notch-border)",
            background: "color-mix(in oklab, var(--notch-text) 6%, transparent)",
            color: copied ? "var(--notch-accent)" : "var(--notch-muted)",
          }}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>

      <div
        className="mt-2.5 h-[2px] w-full overflow-hidden"
        style={{ background: "color-mix(in oklab, var(--notch-text) 8%, transparent)" }}
      >
        <div
          className="h-full"
          style={{
            width: `${Math.max(0, Math.min(1, progress)) * 100}%`,
            background: "var(--notch-accent)",
            opacity: paused ? 0.45 : 1,
          }}
        />
      </div>
    </motion.div>
  );
}
