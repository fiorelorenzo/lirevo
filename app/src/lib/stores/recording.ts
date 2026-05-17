import { readable, type Readable } from 'svelte/store';
import { lda } from '../tauri';

export const recording: Readable<boolean> = readable<boolean>(false, (set) => {
  let unlisten: (() => void) | null = null;
  void lda.onRecordingState((r) => set(r)).then((u) => { unlisten = u; });
  return () => unlisten?.();
});

export const audioLevel: Readable<number> = readable<number>(0, (set) => {
  let unlisten: (() => void) | null = null;
  void lda.onAudioLevel((l) => set(l)).then((u) => { unlisten = u; });
  return () => unlisten?.();
});
