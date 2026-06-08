import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';

// Clear mock call history before each test. Module-level `syncStarted` is
// tested via a fresh dynamic import within the test (see below).
beforeEach(() => {
  vi.mocked(listen).mockClear();
});

describe('startSettingsSync', () => {
  it('updates the settings store when a settings:updated event fires', async () => {
    // Reset module registry so we get a fresh `syncStarted = false` and a
    // fresh `settings` writable for this test.
    vi.resetModules();

    // Re-import the mocked event module AFTER resetModules so we hold the
    // same instance that the freshly-imported settings store will use.
    const { listen: freshListen } = await import('@tauri-apps/api/event');

    let capturedCallback: ((event: { payload: unknown }) => void) | null = null;
    vi.mocked(freshListen).mockImplementation((_event, cb) => {
      capturedCallback = cb as (event: { payload: unknown }) => void;
      return Promise.resolve(() => {});
    });

    const { settings, startSettingsSync } = await import('../stores/settings.svelte');

    await startSettingsSync();

    expect(capturedCallback).not.toBeNull();

    const updatedSettings = {
      sttModelId: 'parakeet-tdt-0.6b-v2',
      whisperModelPath: null,
      llmModelPath: null,
      llmCtxSize: 4096,
      whisperCoreMLDisable: false,
      hotkey: 'right-option' as const,
      language: 'en',
      inputDeviceName: null,
      smartMicRouting: true,
      backupInputDevice: null,
      forcePasteboard: false,
      pasteDelayMs: 120,
      launchAtLogin: false,
      recordHistory: true,
      profileMode: 'auto',
      uiLanguage: 'en',
      onboardingComplete: true,
      appVersion: '0.6.0',
    };

    capturedCallback!({ payload: updatedSettings });

    expect(get(settings)).toEqual(updatedSettings);
    expect(get(settings)!.onboardingComplete).toBe(true);
  });

  it('registers the listener only once even when called multiple times (idempotent)', async () => {
    // Fresh module state for this test.
    vi.resetModules();

    const { listen: freshListen } = await import('@tauri-apps/api/event');
    vi.mocked(freshListen).mockResolvedValue(() => {});

    const { startSettingsSync } = await import('../stores/settings.svelte');

    await startSettingsSync();
    await startSettingsSync();
    await startSettingsSync();

    // Only the first call should have registered the settings:updated listener.
    const settingsListenerCalls = vi
      .mocked(freshListen)
      .mock.calls.filter(([event]) => event === 'settings:updated');
    expect(settingsListenerCalls).toHaveLength(1);
  });
});
