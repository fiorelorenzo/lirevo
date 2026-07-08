import type { LocalModel } from "$lib/tauri";

export interface InstallState {
  installed: boolean;
  sizeBytes: number | null;
}

/** Resolve whether a fixed model is present on disk. STT matches by filename
 * suffix (Parakeet is not in the inference-core catalog, so it appears as a
 * `custom:` local entry); LLM matches by catalog id. */
export function modelInstallState(
  local: LocalModel[],
  match: { filename?: string; id?: string },
): InstallState {
  const hit = local.find(
    (l) =>
      (match.id !== undefined && l.id === match.id) ||
      (match.filename !== undefined && l.path.endsWith(match.filename)),
  );
  return { installed: !!hit, sizeBytes: hit ? hit.sizeBytes : null };
}
