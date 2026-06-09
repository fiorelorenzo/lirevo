import { lda, type ActiveBackendInfo, type ModelState } from '../tauri';

export type BackendState = 'gpu' | 'cpu' | 'resolving';

export interface BackendDisplay {
  label: string;
  state: BackendState;
}

/** Pure mapping from one engine's `(deviceName, isGpu)` pair to a friendly
 *  display label + status. Kept side-effect-free so it can be unit-tested in
 *  isolation (see `__tests__/backend.test.ts`).
 *
 *  - Empty `name` -> the backend hasn't resolved yet (ggml creates it lazily
 *    on the first model load): `resolving`, regardless of `isGpu`.
 *  - Known accelerators get a friendly name (Metal / CUDA / Vulkan); CPU maps
 *    to "CPU"; anything else falls back to the raw device name.
 *  - `state` is `resolving` when empty, `gpu` when `isGpu`, else `cpu`. */
export function deriveBackend(name: string, isGpu: boolean): BackendDisplay {
  if (name === '') {
    return { label: 'Resolving…', state: 'resolving' };
  }
  const lower = name.toLowerCase();
  let label: string;
  if (lower.includes('metal') || lower === 'mtl' || lower.startsWith('mtl')) {
    label = 'Metal GPU';
  } else if (lower.includes('cuda')) {
    label = 'CUDA';
  } else if (lower.includes('vulkan')) {
    label = 'Vulkan';
  } else if (lower === 'cpu') {
    label = 'CPU';
  } else {
    label = name;
  }
  return { label, state: isGpu ? 'gpu' : 'cpu' };
}

const RESOLVING: BackendDisplay = { label: 'Resolving…', state: 'resolving' };

/** Reactive view of the active STT + LLM compute backends. Seeds from
 *  `get_active_backend` at construction, then re-fetches whenever the model
 *  state transitions to Ready — the backend only resolves after the first
 *  model load, so the initial fetch usually returns "resolving". */
class BackendStore {
  stt = $state<BackendDisplay>(RESOLVING);
  llm = $state<BackendDisplay>(RESOLVING);

  /** True when both engines resolved to the same friendly label (the common
   *  case on Apple Silicon, where STT + LLM both land on Metal). The UI uses
   *  this to collapse two rows into one. */
  get unified(): boolean {
    return this.stt.state !== 'resolving' && this.stt.label === this.llm.label;
  }

  constructor() {
    void this.refresh();
    // Re-fetch when the engine reports Ready: the lazy backend is created on
    // the first load, so the value is only meaningful from then on.
    void lda.onModelState((s: ModelState) => {
      if (s.kind === 'ready') {
        void this.refresh();
      }
    });
  }

  async refresh(): Promise<void> {
    try {
      const info = await lda.getActiveBackend();
      this.apply(info);
    } catch {
      // Leave the last-known (or resolving) value; the next Ready event retries.
    }
  }

  private apply(info: ActiveBackendInfo): void {
    this.stt = deriveBackend(info.stt, info.sttIsGpu);
    this.llm = deriveBackend(info.llm, info.llmIsGpu);
  }
}

export const backend = new BackendStore();
