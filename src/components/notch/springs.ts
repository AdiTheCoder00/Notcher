import type { Transition } from "framer-motion";

/** Pill resize: quick, slightly overshooting, settles fast. */
export const SPRING: Transition = { type: "spring", stiffness: 420, damping: 34, mass: 0.9 };

/** Softer spring for content that should not feel bouncy. */
export const SOFT_SPRING: Transition = { type: "spring", stiffness: 260, damping: 30 };
