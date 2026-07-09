import type { CaptureEvent, HotkeySpec, ModifierFlags } from "./hotkey";

/**
 * Release-gated hotkey capture.
 *
 * The Rust event tap streams a live snapshot on every key/modifier/mouse event
 * while recording. The naive approach — commit the instant a base key arrives —
 * makes multi-key combos impossible: the first key wins before the rest of the
 * chord is pressed. Instead we accumulate the richest chord seen during a press
 * and only commit once every key is released (an all-released snapshot).
 */

function modCount(m: ModifierFlags): number {
  return [m.control, m.option, m.command, m.shift, m.fn].filter(Boolean).length;
}

/** A snapshot with nothing held: the user has released every key/button. */
function allReleased(e: CaptureEvent): boolean {
  return e.baseKey == null && e.mouse == null && e.modOnly == null && modCount(e.modifiers) === 0;
}

/** Higher = a more complete chord. A base-key chord beats a mouse button, which
 * beats a lone modifier; among base-key chords, more modifiers wins. */
function richness(s: HotkeySpec): number {
  const t = s.trigger;
  if (typeof t === "object" && "key" in t) return 100 + modCount(s.modifiers);
  if (typeof t === "object" && "mouse" in t) return 50;
  if (typeof t === "object" && "modifierOnly" in t) return 10;
  return 0; // "fn"
}

/** The trigger implied by a single (non-empty) snapshot, or null when it isn't
 * yet a committable chord (e.g. two modifiers held with no base key). */
function candidate(e: CaptureEvent): HotkeySpec | null {
  if (e.baseKey) return { modifiers: e.modifiers, trigger: { key: e.baseKey } };
  if (e.mouse != null) return { modifiers: {}, trigger: { mouse: e.mouse } };
  // A single modifier held on its own is a valid modifier-only PTT trigger.
  if (e.modOnly && modCount(e.modifiers) === 1) {
    return { modifiers: {}, trigger: { modifierOnly: e.modOnly } };
  }
  return null;
}

export interface CaptureState {
  /** Richest chord observed since the current press began. */
  best: HotkeySpec | null;
  /** Whether at least one key/modifier/button has been pressed this round. */
  pressed: boolean;
}

export function initialCaptureState(): CaptureState {
  return { best: null, pressed: false };
}

export interface CaptureStep {
  state: CaptureState;
  /** Set once the user releases everything — the chord to persist. */
  commit: HotkeySpec | null;
}

/**
 * Fold one live capture snapshot into the recorder state. Commits nothing until
 * every key is released, then returns the richest chord seen during the press —
 * which is what lets the user build multi-key combos (Cmd+Shift+D) instead of
 * the first key winning instantly.
 */
export function stepCapture(state: CaptureState, e: CaptureEvent): CaptureStep {
  if (allReleased(e)) {
    if (state.pressed && state.best) {
      return { state: initialCaptureState(), commit: state.best };
    }
    // Nothing was pressed (e.g. an initial empty snapshot) — stay idle.
    return { state, commit: null };
  }
  const cand = candidate(e);
  const best =
    cand && (state.best == null || richness(cand) > richness(state.best)) ? cand : state.best;
  return { state: { best, pressed: true }, commit: null };
}
