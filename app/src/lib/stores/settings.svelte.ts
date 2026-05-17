import { writable, type Writable } from 'svelte/store';
import { lda, type Settings } from '../tauri';

export const settings: Writable<Settings | null> = writable(null);

export async function loadSettings(): Promise<void> {
  settings.set(await lda.getSettings());
}

export async function updateSettings(patch: Partial<Settings>): Promise<void> {
  const next = await lda.updateSettings(patch);
  settings.set(next);
}
