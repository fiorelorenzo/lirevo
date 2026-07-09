/**
 * Model ids whose download the wizard has already kicked off this session.
 * The Language step's "Next" fires the fixed downloads, but the user can
 * navigate back and forward again; without this guard each pass would
 * re-trigger the backend download (a second STT fetch fights hf_hub's blob
 * lock) and re-emit a `queued` progress event that resets the bars.
 * Module-level so it survives the step components re-mounting on navigation.
 * Retry buttons call the download command directly and bypass this guard.
 */
const startedDownloadIds = new Set<string>();

/** Returns true the first time an id is seen (caller should start it then). */
export function markDownloadStarted(id: string): boolean {
  if (startedDownloadIds.has(id)) return false;
  startedDownloadIds.add(id);
  return true;
}
