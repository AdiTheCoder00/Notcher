import { motion } from "framer-motion";

/** The resting mark: three bars, small enough to ignore at the top of the screen. */
export function IdleMark() {
  return (
    <motion.span
      className="flex items-center justify-center"
      style={{ color: "var(--notch-muted)" }}
      initial={{ opacity: 0.55, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ type: "spring", stiffness: 320, damping: 26 }}
      aria-hidden="true"
    >
      <svg width="13" height="13" viewBox="0 0 13 13" fill="currentColor">
        <rect x="0.5" y="5" width="2" height="3" rx="1" />
        <rect x="5.5" y="2" width="2" height="9" rx="1" />
        <rect x="10.5" y="4" width="2" height="5" rx="1" />
      </svg>
    </motion.span>
  );
}
