import { motion } from "framer-motion";

const DOTS = [0, 1, 2];

/** Three dots mid-thought, on a spring-free keyframe loop. */
export function ThinkingDots() {
  return (
    <span className="flex shrink-0 items-center gap-1" aria-hidden="true">
      {DOTS.map((index) => (
        <motion.span
          key={index}
          className="h-1.5 w-1.5 rounded-full"
          style={{ background: "var(--notch-accent)" }}
          animate={{ opacity: [0.25, 1, 0.25], y: [0, -2.5, 0] }}
          transition={{ duration: 1.1, repeat: Infinity, ease: "easeInOut", delay: index * 0.14 }}
        />
      ))}
    </span>
  );
}
