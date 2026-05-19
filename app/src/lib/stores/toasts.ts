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

// Side-effect: forward backend toasts to Sonner. Module is imported by the
// root layout (per-webview); per-window duplication of this listener is
// intentional — each window renders its own Toaster.
void lda.onToast((t) => showToast(t.kind, t.message));
