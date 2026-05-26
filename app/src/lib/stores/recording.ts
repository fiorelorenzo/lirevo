import { readable, type Readable } from 'svelte/store';
import { lda, type PartialTranscript } from '../tauri';

export const recording: Readable<boolean> = readable<boolean>(false, (set) => {
  let unlisten: (() => void) | null = null;
  void lda.onRecordingState((r) => set(r)).then((u) => { unlisten = u; });
  return () => unlisten?.();
});

/** Live cumulative transcript pushed by the streaming STT worker. Empty
 *  string outside a dictation. Consumers in the main window can use this
 *  for live-transcript surfaces; the overlay subscribes to the raw Tauri
 *  event directly (see overlay/+page.svelte) because each webview gets
 *  its own module instance. */
export const partialTranscript: Readable<PartialTranscript> = readable<PartialTranscript>(
  { text: '', delta: '', isFinal: false },
  (set) => {
    let unlisten: (() => void) | null = null;
    void lda.onPartialTranscript((p) => set(p)).then((u) => { unlisten = u; });
    return () => unlisten?.();
  },
);

let audioLevelDebugCount = 0;
export const audioLevel: Readable<number> = readable<number>(0, (set) => {
  let unlisten: (() => void) | null = null;
  console.info('[audioLevel] subscribing to recording:level event');
  void lda
    .onAudioLevel((l) => {
      // Log first 5 and every 30th level so we can confirm events arrive
      // without flooding the console.
      audioLevelDebugCount++;
      if (audioLevelDebugCount <= 5 || audioLevelDebugCount % 30 === 0) {
        console.info(
          `[audioLevel] event #${audioLevelDebugCount}: level=${l.toFixed(4)}`,
        );
      }
      set(l);
    })
    .then((u) => {
      unlisten = u;
      console.info('[audioLevel] listener installed');
    });
  return () => {
    console.info('[audioLevel] unsubscribing (no more consumers)');
    unlisten?.();
  };
});
