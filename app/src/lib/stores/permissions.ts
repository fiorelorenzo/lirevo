import { readable, type Readable } from 'svelte/store';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { lda, type PermissionStatus } from '../tauri';

export interface PermissionsState {
  accessibility: PermissionStatus | null;
  microphone: PermissionStatus | null;
}

// Polls macOS TCC state for Accessibility + Microphone and surfaces it as a
// store the UI can render a permanent banner from. We poll on three signals:
//   1. Once at subscribe (initial value).
//   2. Every 3s while subscribed (covers grants made in System Settings
//      without the user re-focusing the app).
//   3. On window focus (instant feedback when the user comes back from
//      System Settings after toggling a switch).
export const permissionsState: Readable<PermissionsState> = readable<PermissionsState>(
  { accessibility: null, microphone: null },
  (set) => {
    let disposed = false;
    let interval: ReturnType<typeof setInterval> | null = null;
    let unfocus: (() => void) | null = null;

    async function refresh() {
      try {
        const [ax, mic] = await Promise.all([
          lda.checkAccessibility(),
          lda.checkMicrophone(),
        ]);
        if (!disposed) set({ accessibility: ax, microphone: mic });
      } catch {
        // Backend probably not ready yet; next tick will retry.
      }
    }

    void refresh();
    interval = setInterval(refresh, 3000);

    void getCurrentWindow()
      .listen('tauri://focus', () => { void refresh(); })
      .then((u) => {
        if (disposed) u();
        else unfocus = u;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (interval) clearInterval(interval);
      unfocus?.();
    };
  },
);
