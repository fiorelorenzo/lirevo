import { writable } from 'svelte/store';

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
