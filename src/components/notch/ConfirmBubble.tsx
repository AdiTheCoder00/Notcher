import { motion } from "framer-motion";
import { CheckIcon } from "../Icons";

/** A small dark disc with a green check, shown briefly after a copy. */
export function ConfirmBubble({ label }: { label: string }) {
  return (
    <motion.span
      className="notch-confirm-bubble"
      role="status"
      aria-label={label}
      initial={{ opacity: 0, scale: 0.78 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.9 }}
      transition={{ type: "spring", stiffness: 420, damping: 30 }}
    >
      <CheckIcon size={15} />
    </motion.span>
  );
}
