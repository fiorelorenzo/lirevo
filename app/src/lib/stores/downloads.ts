import { writable, derived, type Readable } from "svelte/store";
import { lda, type DownloadProgress } from "../tauri";

const _downloads = writable<Record<string, DownloadProgress>>({});
export const downloads = { subscribe: _downloads.subscribe };

export function progressFor(id: string): Readable<DownloadProgress | undefined> {
  return derived(_downloads, ($d) => $d[id]);
}

void lda.onDownloadProgress((p) => {
  _downloads.update((d) => ({ ...d, [p.id]: p }));
});
