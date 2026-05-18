import { writable } from 'svelte/store';
import { lda } from '../tauri';

export interface Toast { id: number; kind: 'info' | 'warn' | 'error'; message: string; }

let nextId = 1;
const _toasts = writable<Toast[]>([]);
export const toasts = { subscribe: _toasts.subscribe };

export function showToast(kind: Toast['kind'], message: string, ttlMs = 4000): void {
  const id = nextId++;
  _toasts.update((arr) => [...arr, { id, kind, message }]);
  setTimeout(() => dismissToast(id), kind === 'error' ? ttlMs * 1.5 : ttlMs);
}

export function dismissToast(id: number): void {
  _toasts.update((arr) => arr.filter((t) => t.id !== id));
}

// Side-effect: wire backend toast events to local queue.
void lda.onToast((t) => showToast(t.kind, t.message));
