import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { settings, startSettingsSync } from "../stores/settings.svelte";

// `listen` is mocked once, globally, in vitest.setup.ts and is therefore a
// single spy shared by every test (`vi.resetModules()` does NOT reset the mocks
// registry). The store's `syncStarted` guard is module-level state that only
// flips once per process, so the two behaviours under test — the first call
// registering + wiring the store, and subsequent calls being no-ops — are two
// facets of one lifecycle. We exercise them in a single test against one store
// instance rather than re-importing the module per test: re-evaluating the
// store's graph via `vi.resetModules()` + dynamic `import()` raced under
// jsdom/vite-node (leaked mock impl, duplicate module instances), which made
// this suite flaky once the run order was shuffled.
beforeEach(() => {
  settings.set(null);
  vi.mocked(listen).mockReset();
});

describe("startSettingsSync", () => {
  it("registers the settings:updated listener once and pipes payloads into the store", async () => {
    let capturedCallback: ((event: { payload: unknown }) => void) | null = null;
    vi.mocked(listen).mockImplementation((_event, cb) => {
      capturedCallback = cb as (event: { payload: unknown }) => void;
      return Promise.resolve(() => {});
    });

    await startSettingsSync();

    // Idempotent: extra calls must not register the listener again.
    await startSettingsSync();
    await startSettingsSync();

    const settingsListenerCalls = vi
      .mocked(listen)
      .mock.calls.filter(([event]) => event === "settings:updated");
    expect(settingsListenerCalls).toHaveLength(1);

    // The single registered callback drives the settings store.
    expect(capturedCallback).not.toBeNull();

    const updatedSettings = {
      llmCtxSize: 4096,
      hotkey: {
        modifiers: {},
        trigger: { modifierOnly: { modifier: "option", side: "right" } },
      } as const,
      activationMode: "hold" as const,
      language: "en",
      inputDeviceName: null,
      smartMicRouting: true,
      backupInputDevice: null,
      pasteDelayMs: 120,
      launchAtLogin: false,
      startMinimized: false,
      recordHistory: true,
      profileMode: "auto",
      uiLanguage: "en",
      onboardingComplete: true,
      appVersion: "0.6.0",
    };

    capturedCallback!({ payload: updatedSettings });

    expect(get(settings)).toEqual(updatedSettings);
    expect(get(settings)!.onboardingComplete).toBe(true);
  });
});
