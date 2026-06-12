import { describe, it, expect } from 'vitest';
import { formatHotkey, validateHotkey, type HotkeySpec } from '$lib/hotkey';

const mac = 'macos' as const;

describe('formatHotkey', () => {
  it('renders a combo with mac glyphs', () => {
    const spec: HotkeySpec = { modifiers: { control: true, shift: true }, trigger: { key: 'K' } };
    expect(formatHotkey(spec, mac)).toEqual(['⌃', '⇧', 'K']);
  });
  it('renders a right modifier-only', () => {
    const spec: HotkeySpec = { modifiers: {}, trigger: { modifierOnly: { modifier: 'option', side: 'right' } } };
    expect(formatHotkey(spec, mac)).toEqual(['⌥ right']);
  });
  it('renders Fn and mouse', () => {
    expect(formatHotkey({ modifiers: {}, trigger: 'fn' }, mac)).toEqual(['fn']);
    expect(formatHotkey({ modifiers: {}, trigger: { mouse: 4 } }, mac)).toEqual(['Mouse 4']);
  });
});

describe('validateHotkey', () => {
  it('rejects a bare alphanumeric', () => {
    expect(validateHotkey({ modifiers: {}, trigger: { key: 'K' } }, mac).ok).toBe(false);
  });
  it('accepts a combo', () => {
    expect(validateHotkey({ modifiers: { command: true, shift: true }, trigger: { key: 'K' } }, mac).ok).toBe(true);
  });
  it('rejects > 3 keys', () => {
    const spec: HotkeySpec = { modifiers: { control: true, command: true, shift: true }, trigger: { key: 'K' } };
    expect(validateHotkey(spec, mac).ok).toBe(false);
  });
  it('rejects a critical system shortcut (Cmd+Q)', () => {
    expect(validateHotkey({ modifiers: { command: true }, trigger: { key: 'Q' } }, mac).ok).toBe(false);
  });
  it('rejects Fn / mouse off macOS', () => {
    expect(validateHotkey({ modifiers: {}, trigger: 'fn' }, 'linux').ok).toBe(false);
    expect(validateHotkey({ modifiers: {}, trigger: { mouse: 4 } }, 'windows').ok).toBe(false);
  });
});
