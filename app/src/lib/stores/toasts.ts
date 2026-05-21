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

export interface ToastOptions {
  /**
   * Auto-dismiss time in milliseconds. `Infinity` keeps the toast on
   * screen until the user dismisses it manually (only useful with
   * `closeButton: true`). Omit to use the per-kind default.
   */
  duration?: number;
  /**
   * Show the X close button. Pairs naturally with `duration: Infinity`
   * — a toast that doesn't dismiss itself needs a way for the user to
   * dismiss it.
   */
  closeButton?: boolean;
}

// Per-kind defaults. The intent:
//   - info + success are acknowledgements that auto-dismiss without an
//     X — they're not blocking news and an X would invite unnecessary
//     interaction for a transient message.
//   - warn keeps the X (warnings often have actionable follow-ups the
//     user may want to read longer) but still auto-dismisses on a
//     longer fuse.
//   - error never auto-dismisses; it requires the X. A failure that
//     vanishes before it's read can't be acted on, and "what did that
//     red toast say?" is exactly the diagnostic chain we've been
//     trying to break in this codebase.
const KIND_DEFAULTS: Record<ToastKind, Required<ToastOptions>> = {
  info: { duration: 4000, closeButton: false },
  success: { duration: 4000, closeButton: false },
  warn: { duration: 6000, closeButton: true },
  error: { duration: Number.POSITIVE_INFINITY, closeButton: true },
};

function resolveOpts(kind: ToastKind, override?: ToastOptions): Required<ToastOptions> {
  return { ...KIND_DEFAULTS[kind], ...override };
}

export function showToast(kind: ToastKind, message: string, opts?: ToastOptions): void {
  const { duration, closeButton } = resolveOpts(kind, opts);
  const sonnerOpts = { duration, closeButton };
  if (kind === 'info') toast.info(message, sonnerOpts);
  else if (kind === 'warn') toast.warning(message, sonnerOpts);
  else if (kind === 'success') toast.success(message, sonnerOpts);
  else toast.error(message, sonnerOpts);
}

// Named convenience wrappers. Use these at call sites for less visual
// noise than `showToast('error', ...)` and to make grepping for a
// specific kind easy. Each accepts the same optional override bag.
export const toastInfo = (message: string, opts?: ToastOptions): void =>
  showToast('info', message, opts);
export const toastWarn = (message: string, opts?: ToastOptions): void =>
  showToast('warn', message, opts);
export const toastError = (message: string, opts?: ToastOptions): void =>
  showToast('error', message, opts);
export const toastSuccess = (message: string, opts?: ToastOptions): void =>
  showToast('success', message, opts);

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
