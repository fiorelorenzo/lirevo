import { writable } from "svelte/store";

/**
 * Handoff between the wizard's Language step and its Downloads step.
 *
 * The Language step resolves the two model ids it kicks off
 * (`sttModelId` is also persisted to settings; `llmId` lives only here),
 * and the Downloads step reads them to know which `download:progress`
 * streams to watch. Kept as a tiny module-level store rather than threaded
 * through props because the two steps are siblings under the wizard
 * router, not parent/child.
 */
export interface WizardDownloadSelection {
  sttId: string | null;
  llmId: string | null;
}

export const wizardDownloadSelection = writable<WizardDownloadSelection>({
  sttId: null,
  llmId: null,
});

/**
 * Model ids whose download the wizard has already kicked off this session.
 * The Language step's "Next" fires the downloads, but the user can navigate
 * back to it and forward again; without this guard each pass would re-trigger
 * the backend download (a second STT fetch fights hf_hub's blob lock) and
 * re-emit a `queued` progress event that resets the bars. Module-level so it
 * survives the step components re-mounting on navigation. Retry buttons call
 * the download command directly and bypass this guard.
 */
const startedDownloadIds = new Set<string>();

/** Returns true the first time an id is seen (caller should start it then). */
export function markDownloadStarted(id: string): boolean {
  if (startedDownloadIds.has(id)) return false;
  startedDownloadIds.add(id);
  return true;
}
