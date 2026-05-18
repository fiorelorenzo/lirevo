import { describe, it, expect, beforeAll } from 'vitest';
import { initI18n, t } from '../i18n';

beforeAll(async () => {
  await initI18n('en');
});

describe('i18n', () => {
  it('returns translation for known key', () => {
    expect(t('common.ok')).toBe('OK');
  });
  it('interpolates parameters', () => {
    expect(t('home.ready_body', { hotkey: 'Right ⌥' })).toContain('Right ⌥');
  });
  it('returns key when translation missing', () => {
    expect(t('nonexistent.key')).toBe('nonexistent.key');
  });
});
