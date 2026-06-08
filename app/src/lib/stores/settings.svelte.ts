import { writable, type Writable } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import { lda, type Settings } from '../tauri';
import { showToast } from './toasts';

export const settings: Writable<Settings | null> = writable(null);

let syncStarted = false;

/** Subscribe to backend `settings:updated` broadcasts so every window's
 * settings store stays in sync (e.g. after the wizard completes in another
 * window). Idempotent — safe to call from multiple layout mounts. */
export async function startSettingsSync(): Promise<void> {
  if (syncStarted) return;
  syncStarted = true;
  await listen<Settings>('settings:updated', (event) => {
    settings.set(event.payload);
  });
}

export async function loadSettings(): Promise<void> {
  settings.set(await lda.getSettings());
}

/**
 * Persist a settings patch. On failure, the cached settings store is left
 * unchanged AND an error toast is shown (so the user sees that their toggle
 * / slider didn't stick instead of the UI silently snapping back). Returns
 * the resolved Settings on success, or null on failure — callers can branch
 * but most just fire-and-forget the toggle.
 */
export async function updateSettings(patch: Partial<Settings>): Promise<Settings | null> {
  try {
    const next = await lda.updateSettings(patch);
    settings.set(next);
    return next;
  } catch (e) {
    const reason = e instanceof Error ? e.message : String(e);
    showToast('error', `Save settings failed: ${reason}`);
    return null;
  }
}
