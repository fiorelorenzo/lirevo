export type Os = "macos" | "windows" | "linux";
export type Modifier = "control" | "option" | "command" | "shift";
export type Side = "left" | "right";

export interface ModifierFlags {
  control?: boolean;
  option?: boolean;
  command?: boolean;
  shift?: boolean;
  fn?: boolean;
}

export type Trigger =
  { key: string } | "fn" | { modifierOnly: { modifier: Modifier; side: Side } } | { mouse: number };

export interface HotkeySpec {
  modifiers: ModifierFlags;
  trigger: Trigger;
}

export type ActivationMode = "hold" | "tap";

export interface CaptureEvent {
  modifiers: ModifierFlags;
  baseKey: string | null;
  modOnly: { modifier: Modifier; side: Side } | null;
  mouse: number | null;
}

const MAC_GLYPH: Record<Modifier, string> = { control: "⌃", option: "⌥", command: "⌘", shift: "⇧" };
const OTHER_LABEL: Record<Modifier, string> = {
  control: "Ctrl",
  option: "Alt",
  command: "Super",
  shift: "Shift",
};

function modChips(m: ModifierFlags, os: Os): string[] {
  const order: Modifier[] = ["control", "option", "command", "shift"];
  const chips = order
    .filter((k) => m[k])
    .map((k) => (os === "macos" ? MAC_GLYPH[k] : OTHER_LABEL[k]));
  if (m.fn) chips.unshift("fn");
  return chips;
}

export function formatHotkey(spec: HotkeySpec, os: Os): string[] {
  const t = spec.trigger;
  if (t === "fn") return ["fn"];
  if ("mouse" in t) return [`Mouse ${t.mouse}`];
  if ("modifierOnly" in t) {
    const { modifier, side } = t.modifierOnly;
    const base = os === "macos" ? MAC_GLYPH[modifier] : OTHER_LABEL[modifier];
    return [`${base} ${side}`];
  }
  return [...modChips(spec.modifiers, os), t.key];
}

export interface ValidationResult {
  ok: boolean;
  error?: string;
}

const MAC_RESERVED = new Set([
  "command+Q",
  "command+W",
  "command+Space",
  "command+Tab",
  "command+H",
  "command+M",
  "command+shift+3",
  "command+shift+4",
  "command+shift+5",
]);

function modCount(m: ModifierFlags): number {
  return [m.control, m.option, m.command, m.shift, m.fn].filter(Boolean).length;
}

function reservedKey(spec: HotkeySpec): string {
  if (typeof spec.trigger !== "object" || !("key" in spec.trigger)) return "";
  const m = spec.modifiers;
  const parts: string[] = [];
  if (m.control) parts.push("control");
  if (m.option) parts.push("option");
  if (m.command) parts.push("command");
  if (m.shift) parts.push("shift");
  parts.push(spec.trigger.key);
  return parts.join("+");
}

export function validateHotkey(spec: HotkeySpec, os: Os): ValidationResult {
  const t = spec.trigger;
  if ((t === "fn" || (typeof t === "object" && "mouse" in t)) && os !== "macos") {
    return { ok: false, error: "Fn and mouse buttons are macOS-only" };
  }
  if (modCount(spec.modifiers) + 1 > 3) {
    return { ok: false, error: "Use at most 3 keys" };
  }
  if (typeof t === "object" && "key" in t) {
    const alnum = t.key.length === 1 && /[A-Za-z0-9]/.test(t.key);
    if (alnum && modCount(spec.modifiers) === 0) {
      return { ok: false, error: "A plain key needs a modifier" };
    }
    if (os === "macos" && MAC_RESERVED.has(reservedKey(spec))) {
      return { ok: false, error: "That shortcut is reserved by macOS" };
    }
  }
  return { ok: true };
}
