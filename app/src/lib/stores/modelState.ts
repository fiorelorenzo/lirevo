import { readable, type Readable } from 'svelte/store';
import { lda, type ModelState } from '../tauri';

// Backend may emit `model:state` events before the subscription below
// finishes registering — at app launch the load_models task spawns
// immediately while the frontend's layout mount is still async. We poll the
// current state once at subscribe time so the store never gets stuck on the
// `idle` default when the real state already moved past it.
export const modelState: Readable<ModelState> = readable<ModelState>({ kind: 'idle' }, (set) => {
  let unlisten: (() => void) | null = null;
  let disposed = false;

  void lda.getModelState().then((s) => { if (!disposed) set(s); }).catch(() => {});
  void lda.onModelState((s) => set(s)).then((u) => {
    if (disposed) { u(); } else { unlisten = u; }
  });

  return () => {
    disposed = true;
    unlisten?.();
  };
});
