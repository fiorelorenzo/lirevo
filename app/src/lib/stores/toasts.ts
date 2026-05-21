// Centralized toast surface for the whole app. Application code MUST go
// through this module — never `import { toast } from 'svelte-sonner'`
// directly. Two reasons:
//   1. Single point to change the underlying library (e.g. swap sonner
//      for a custom Tauri-native notification surface) without grepping
//      every call site.
//   2. Backend `toast` events (emitted from Rust via `app.emit("toast", …)`)
//      flow through the same pipe as frontend-originated toasts, so the
//      visual treatment + queue + dedupe behavior is identical
//      regardless of origin.
//
// The previous design had a local writable store mirroring the events
// plus a +layout.svelte subscriber forwarding into Sonner — two queues
// for the same payload, with an unbounded `shown` Set leak in the
// layout. Now there's exactly one Sonner instance per webview.
import { toast } from 'svelte-sonner';
import { lda } from '../tauri';

export type ToastKind = 'info' | 'warn' | 'error' | 'success';

export function showToast(kind: ToastKind, message: string): void {
  if (kind === 'info') toast.info(message);
  else if (kind === 'warn') toast.warning(message);
  else if (kind === 'success') toast.success(message);
  else toast.error(message);
}

// Named convenience wrappers. Use these at call sites for less visual
// noise than `showToast('error', ...)` and to make grepping for a
// specific kind easy.
export const toastInfo = (message: string): void => showToast('info', message);
export const toastWarn = (message: string): void => showToast('warn', message);
export const toastError = (message: string): void => showToast('error', message);
export const toastSuccess = (message: string): void => showToast('success', message);

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
