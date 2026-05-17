import { readable, type Readable } from 'svelte/store';
import { lda, type ModelState } from '../tauri';

export const modelState: Readable<ModelState> = readable<ModelState>({ kind: 'idle' }, (set) => {
  let unlisten: (() => void) | null = null;
  void lda.onModelState((s) => set(s)).then((u) => { unlisten = u; });
  return () => unlisten?.();
});
