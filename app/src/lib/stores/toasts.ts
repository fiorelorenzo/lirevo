// Thin wrapper around svelte-sonner so the rest of the app calls a single
// `showToast(kind, message)` instead of toast.info/warn/error directly.
// Also wires backend `toast` events (emitted from Rust) straight into
// Sonner — there used to be a local writable store mirroring the events
// then a +layout.svelte subscriber forwarding into Sonner; two queues
// for the same payload that grew an unbounded `shown` Set in the layout.
import { toast } from 'svelte-sonner';
import { lda } from '../tauri';

export type ToastKind = 'info' | 'warn' | 'error';

export function showToast(kind: ToastKind, message: string): void {
  if (kind === 'info') toast.info(message);
  else if (kind === 'warn') toast.warning(message);
  else toast.error(message);
}

/**
 * Run an async operation; on failure, surface the error as a toast with the
 * given action label. Returns the resolved value on success, or `null` on
 * failure (so callers can branch without a second try/catch).
 *
 * Use this instead of a bare `try { ... } catch (e) { console.error(...) }`
 * for any operation the user explicitly triggered — silent console errors
 * read as "the app is broken" because the user pressed a button and saw
 * nothing happen. Toast format: `${label} failed: ${reason}`.
 *
 * Tauri command errors arrive as the `AppError`'s string form. We pass them
 * through verbatim under the assumption the backend already wrote a useful
 * message (e.g. "Uninstall failed: model file not at expected path ...").
 * For non-Error throws we fall back to `String(e)`.
 */
export async function withErrorToast<T>(
  label: string,
  op: () => Promise<T>,
): Promise<T | null> {
  try {
    return await op();
  } catch (e) {
    const reason = e instanceof Error ? e.message : String(e);
    showToast('error', `${label} failed: ${reason}`);
    return null;
  }
}

// Side-effect: forward backend toasts to Sonner. Module is imported by the
// root layout (per-webview); per-window duplication of this listener is
// intentional — each window renders its own Toaster.
void lda.onToast((t) => showToast(t.kind, t.message));
