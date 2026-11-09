import { motion } from "framer-motion";

/** The resting mark: three bars, small enough to ignore at the top of the screen. */
export function IdleMark() {
  return (
    <motion.span
      className="notch-idle-mark"
      style={{ color: "var(--notch-muted)" }}
      initial={{ opacity: 0.55, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ type: "spring", stiffness: 320, damping: 26 }}
      aria-hidden="true"
    >
      <svg width="18" height="14" viewBox="0 0 18 14" fill="currentColor">
        <rect x="1" y="5.5" width="2" height="3" rx="1" opacity=".55" />
        <rect x="4.5" y="3.5" width="2" height="7" rx="1" opacity=".8" />
        <rect x="8" y="1.5" width="2" height="11" rx="1" />
        <rect x="11.5" y="3.5" width="2" height="7" rx="1" opacity=".8" />
        <rect x="15" y="5.5" width="2" height="3" rx="1" opacity=".55" />
      </svg>
    </motion.span>
  );
}
