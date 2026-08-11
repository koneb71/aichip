import type { Transition, Variants } from "framer-motion";

/**
 * How this app moves.
 *
 * One file, because motion that disagrees with itself reads as jank rather
 * than as personality: a card that springs while its neighbour eases looks
 * broken even when both look fine alone. Everything here is exported as a
 * variant or a transition so a component picks a *named* behaviour instead of
 * inventing numbers.
 *
 * Two rules the whole set obeys:
 *
 * - **Nothing animates layout.** Only `transform` and `opacity`, which the
 *   compositor can do without touching the main thread. Animating height or
 *   top is how a list of thirty cards drops frames on a laptop.
 * - **Entrances are short and exits are shorter.** An exit is time between the
 *   user's click and what they asked for; an entrance is time they are already
 *   spending looking at something. 240ms in, 140ms out.
 */

/** The house curve, matching `--ease-out-soft` in index.css. */
export const EASE = [0.22, 1, 0.36, 1] as const;

export const fast: Transition = { duration: 0.14, ease: EASE };
export const base: Transition = { duration: 0.24, ease: EASE };
export const slow: Transition = { duration: 0.42, ease: EASE };

/**
 * For anything the pointer is pushing around.
 *
 * A spring rather than a curve, because a press should feel like it has mass —
 * the duration then comes from the physics rather than from a number that has
 * to be re-guessed every time the distance changes.
 */
export const springy: Transition = { type: "spring", stiffness: 400, damping: 30 };
export const springSoft: Transition = { type: "spring", stiffness: 220, damping: 26 };

/** A page arriving. Paired with `AnimatePresence mode="wait"` in the shell. */
export const pageVariants: Variants = {
  hidden: { opacity: 0, y: 8 },
  show: { opacity: 1, y: 0, transition: { ...base, when: "beforeChildren" } },
  exit: { opacity: 0, y: -4, transition: fast },
};

/**
 * A container whose children arrive one after another.
 *
 * `staggerChildren` is deliberately small: 40ms reads as one movement with
 * texture, 150ms reads as a queue you are waiting on. `delayChildren` gives
 * the container's own fade a moment to start first, so the group feels like it
 * arrives together rather than the first child racing ahead.
 */
export const listVariants: Variants = {
  hidden: { opacity: 0 },
  show: {
    opacity: 1,
    transition: { staggerChildren: 0.04, delayChildren: 0.04 },
  },
};

/** One child of a `listVariants` container. */
export const itemVariants: Variants = {
  hidden: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: base },
};

/** A tile that pops rather than slides — for the icon row on the home hero. */
export const tileVariants: Variants = {
  hidden: { opacity: 0, y: 12, scale: 0.94 },
  show: { opacity: 1, y: 0, scale: 1, transition: springSoft },
};

/** A modal and its backdrop. */
export const overlayVariants: Variants = {
  hidden: { opacity: 0 },
  show: { opacity: 1, transition: base },
  exit: { opacity: 0, transition: fast },
};

export const dialogVariants: Variants = {
  hidden: { opacity: 0, y: 12, scale: 0.97 },
  show: { opacity: 1, y: 0, scale: 1, transition: springSoft },
  exit: { opacity: 0, y: 8, scale: 0.98, transition: fast },
};

/** A panel sliding in from the right. */
export const drawerVariants: Variants = {
  hidden: { x: "100%" },
  show: { x: 0, transition: springSoft },
  exit: { x: "100%", transition: { duration: 0.2, ease: EASE } },
};

/** What a pressable thing does under the pointer. Spread onto a motion element. */
export const pressable = {
  whileHover: { y: -1 },
  whileTap: { scale: 0.97 },
  transition: springy,
} as const;

/** Same, for something large enough that lifting it would look heavy. */
export const tappable = {
  whileTap: { scale: 0.98 },
  transition: springy,
} as const;
