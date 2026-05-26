import { invoke } from '@tauri-apps/api/core';

export type LanguageCoverage = 'european_25' | 'global_30' | 'multilingual_99';

export type FeatureRequirement = 'always' | 'audiopipe_whisper_feature';

/**
 * Frontend mirror of `app/src-tauri/src/stt/catalog.rs`. Keep the two
 * lists in lockstep — the backend is the source of truth and the dev-only
 * `assertCatalogParity()` below catches drift before it ships.
 *
 * `coreml_url` (from the pre-M4 catalog) is intentionally absent —
 * audiopipe resolves weights from the HF cache transparently.
 */
export interface SttModelEntry {
  id: string;
  displayName: string;
  sizeBytes: number;
  languageCoverage: LanguageCoverage;
  /** One-line summary shown under the display name in the wizard radio card. */
  summary: string;
  /** SPDX-style license label rendered as a small badge. */
  license: string;
  /**
   * ISO 639-1/2 codes the model can decode. For
   * `language_coverage = 'multilingual_99'` the list is the single
   * placeholder `'multilingual-99'`, and the Language wizard step expands
   * it to a curated subset rather than dumping the full ~99-entry list.
   */
  languages: string[];
  /** True for the entry that is pre-selected on a fresh install. */
  default: boolean;
  featureRequirement: FeatureRequirement;
}

const PARAKEET_LANGUAGES = [
  'en', 'it', 'de', 'fr', 'es', 'pt', 'nl', 'pl', 'ru', 'uk', 'cs', 'hr',
  'bg', 'da', 'el', 'et', 'fi', 'hu', 'lv', 'lt', 'mt', 'ro', 'sk', 'sl',
  'sv',
];

const QWEN3_LANGUAGES = [
  'zh', 'en', 'yue', 'ar', 'de', 'fr', 'es', 'pt', 'id', 'it', 'ko', 'ru',
  'th', 'vi', 'ja', 'tr', 'hi', 'ms', 'nl', 'sv', 'da', 'fi', 'pl', 'cs',
  'fil', 'fa', 'el', 'hu', 'mk', 'ro',
];

export const STT_MODELS: SttModelEntry[] = [
  {
    id: 'parakeet-tdt-0.6b-v3',
    displayName: 'Parakeet TDT v3',
    sizeBytes: 600_000_000,
    languageCoverage: 'european_25',
    summary: '25 European languages incl. Italian. Lowest latency.',
    license: 'CC-BY-4.0',
    languages: PARAKEET_LANGUAGES,
    default: true,
    featureRequirement: 'always',
  },
  {
    id: 'qwen3-asr-0.6b-ggml',
    displayName: 'Qwen3-ASR (broad languages)',
    sizeBytes: 700_000_000,
    languageCoverage: 'global_30',
    summary: '30 languages + 22 Chinese dialects. Adds JA/ZH/AR/HI.',
    license: 'Apache-2.0',
    languages: QWEN3_LANGUAGES,
    default: false,
    featureRequirement: 'always',
  },
  {
    id: 'whisper-large-v3-turbo',
    displayName: 'Whisper large-v3-turbo (other languages)',
    sizeBytes: 1_500_000_000,
    languageCoverage: 'multilingual_99',
    summary: '99 languages. Slower but broadest coverage.',
    license: 'MIT/Apache-2.0',
    languages: ['multilingual-99'],
    default: false,
    featureRequirement: 'audiopipe_whisper_feature',
  },
];

export function defaultModelId(): string {
  const def = STT_MODELS.find((m) => m.default);
  if (!def) {
    throw new Error('STT_MODELS catalog has no default entry');
  }
  return def.id;
}

export function findModel(id: string): SttModelEntry | undefined {
  return STT_MODELS.find((m) => m.id === id);
}

/**
 * Curated Whisper language subset shown in the wizard's language step when
 * the user picks the Whisper model. The full Whisper list is ~99 entries —
 * dumping it raw into a dropdown is unusable, so we surface the
 * most-spoken languages first (with the few the other two models cover
 * anyway at the top for parity) and let the user fall back to `auto-detect`
 * for anything not listed.
 *
 * Grouped roughly by region for visual scanning; the dropdown renders
 * them as a single flat list ordered by appearance here.
 */
export const WHISPER_CURATED_LANGUAGES: string[] = [
  // Top European (also covered by Parakeet)
  'en', 'it', 'de', 'fr', 'es', 'pt', 'nl',
  // Slavic + Nordic / Eastern European
  'pl', 'ru', 'uk', 'cs', 'sv', 'da', 'fi', 'no', 'el', 'ro', 'hu',
  // Asian (the Whisper-only differentiators)
  'zh', 'ja', 'ko', 'hi', 'th', 'vi', 'id', 'ms',
  // Middle East
  'ar', 'fa', 'he', 'tr',
  // Other
  'sw',
];

/**
 * Resolve the language list to show in the wizard language step for a
 * given model. Whisper expands its `multilingual-99` placeholder to
 * [`WHISPER_CURATED_LANGUAGES`]; everything else uses its own catalog list.
 */
export function languagesForModel(id: string): string[] {
  const m = findModel(id);
  if (!m) return [];
  if (m.languageCoverage === 'multilingual_99') {
    return [...WHISPER_CURATED_LANGUAGES];
  }
  return [...m.languages];
}

// ---------- Dev-only parity check ----------

/**
 * Wire shape returned by the `get_stt_catalog` Tauri command. Matches
 * `app/src-tauri/src/stt/catalog.rs::Metadata` serialized as camelCase.
 */
interface BackendCatalogEntry {
  id: string;
  displayName: string;
  sizeBytes: number;
  languageCoverage: LanguageCoverage;
  summary: string;
  license: string;
  languages: string[];
  default: boolean;
  featureRequirement: FeatureRequirement;
}

/**
 * Debug-only: fetch the backend catalog and panic if the TS mirror has
 * drifted. The frontend always reads from the static `STT_MODELS` array
 * for speed and SSR ergonomics — this check just guarantees that the
 * static array doesn't lie about what the backend can actually load.
 *
 * Production builds skip the check entirely (zero network, zero IPC).
 */
export async function assertCatalogParity(): Promise<void> {
  if (!import.meta.env.DEV) return;
  let backend: BackendCatalogEntry[];
  try {
    backend = await invoke<BackendCatalogEntry[]>('get_stt_catalog');
  } catch (e) {
    // Tauri unavailable (e.g. running vitest under jsdom) — treat as no-op.
    // The Rust-side unit tests are the canonical guarantee.
    console.debug('[stt-catalog] backend probe failed, skipping parity check:', e);
    return;
  }
  if (backend.length !== STT_MODELS.length) {
    throw new Error(
      `[stt-catalog] frontend has ${STT_MODELS.length} entries, backend has ${backend.length}`,
    );
  }
  for (const fe of STT_MODELS) {
    const be = backend.find((b) => b.id === fe.id);
    if (!be) {
      throw new Error(`[stt-catalog] frontend entry '${fe.id}' missing from backend`);
    }
    const fields: (keyof SttModelEntry & keyof BackendCatalogEntry)[] = [
      'displayName', 'sizeBytes', 'languageCoverage', 'summary', 'license',
      'default', 'featureRequirement',
    ];
    for (const f of fields) {
      if (fe[f] !== be[f]) {
        throw new Error(
          `[stt-catalog] '${fe.id}'.${f} mismatch — frontend=${JSON.stringify(fe[f])}, backend=${JSON.stringify(be[f])}`,
        );
      }
    }
    if (fe.languages.length !== be.languages.length
      || fe.languages.some((l, i) => l !== be.languages[i])) {
      throw new Error(
        `[stt-catalog] '${fe.id}'.languages mismatch — frontend=${JSON.stringify(fe.languages)}, backend=${JSON.stringify(be.languages)}`,
      );
    }
  }
}

/**
 * Format a byte count like `600 MB` or `1.5 GB`. Mirrors the helper used
 * by the legacy `ModelCard` so the wizard and settings cards look
 * consistent.
 */
export function formatSize(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}
