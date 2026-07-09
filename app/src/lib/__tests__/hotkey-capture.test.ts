import { describe, it, expect } from "vitest";
import { initialCaptureState, stepCapture } from "../hotkey-capture";
import type { CaptureEvent, HotkeySpec } from "../hotkey";

const empty: CaptureEvent = { modifiers: {}, baseKey: null, modOnly: null, mouse: null };

/** Feed a sequence of live snapshots through the reducer; return the committed
 * spec (last non-null commit) or null. */
function run(events: CaptureEvent[]): HotkeySpec | null {
  let state = initialCaptureState();
  let committed: HotkeySpec | null = null;
  for (const e of events) {
    const r = stepCapture(state, e);
    state = r.state;
    if (r.commit) committed = r.commit;
  }
  return committed;
}

describe("hotkey capture (wait-for-release)", () => {
  it("builds a multi-key chord and commits only on full release", () => {
    // Cmd ↓, Shift ↓, D ↓, D ↑, Shift ↑, Cmd ↑
    const spec = run([
      {
        modifiers: { command: true },
        baseKey: null,
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true, shift: true },
        baseKey: null,
        modOnly: { modifier: "shift", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true, shift: true },
        baseKey: "D",
        modOnly: { modifier: "shift", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true, shift: true },
        baseKey: null,
        modOnly: { modifier: "shift", side: "left" },
        mouse: null,
      },
      { modifiers: { command: true }, baseKey: null, modOnly: null, mouse: null },
      empty,
    ]);
    expect(spec).toEqual({ modifiers: { command: true, shift: true }, trigger: { key: "D" } });
  });

  it("does not commit while keys are still held", () => {
    let state = initialCaptureState();
    for (const e of [
      {
        modifiers: { command: true },
        baseKey: null,
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true },
        baseKey: "D",
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
    ] as CaptureEvent[]) {
      const r = stepCapture(state, e);
      state = r.state;
      expect(r.commit).toBeNull();
    }
  });

  it("never commits the lone modifier early when a key follows (the bug)", () => {
    // Cmd ↓ (slowly) then D — must yield Cmd+D, never Cmd alone.
    const spec = run([
      {
        modifiers: { command: true },
        baseKey: null,
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true },
        baseKey: "D",
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
      {
        modifiers: { command: true },
        baseKey: null,
        modOnly: { modifier: "command", side: "left" },
        mouse: null,
      },
      empty,
    ]);
    expect(spec).toEqual({ modifiers: { command: true }, trigger: { key: "D" } });
  });

  it("commits a lone modifier as a modifier-only trigger on release", () => {
    const spec = run([
      {
        modifiers: { option: true },
        baseKey: null,
        modOnly: { modifier: "option", side: "right" },
        mouse: null,
      },
      empty,
    ]);
    expect(spec).toEqual({
      modifiers: {},
      trigger: { modifierOnly: { modifier: "option", side: "right" } },
    });
  });

  it("commits a plain function key with no modifiers", () => {
    const spec = run([{ modifiers: {}, baseKey: "F5", modOnly: null, mouse: null }, empty]);
    expect(spec).toEqual({ modifiers: {}, trigger: { key: "F5" } });
  });

  it("commits a mouse button on release", () => {
    const spec = run([
      { modifiers: {}, baseKey: null, modOnly: null, mouse: 4 },
      { modifiers: {}, baseKey: null, modOnly: null, mouse: null },
    ]);
    expect(spec).toEqual({ modifiers: {}, trigger: { mouse: 4 } });
  });

  it("ignores a leading empty snapshot before any press", () => {
    const r = stepCapture(initialCaptureState(), empty);
    expect(r.commit).toBeNull();
    expect(r.state.pressed).toBe(false);
  });
});
