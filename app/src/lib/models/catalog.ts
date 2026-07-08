import { invoke } from "@tauri-apps/api/core";

export type LanguageCoverage = "european_25" | "global_30" | "multilingual_99";

/**
 * Frontend mirror of `app/src-tauri/src/stt/catalog.rs`. Keep the two
 * lists in lockstep — the backend is the source of truth and the dev-only
 * `assertCatalogParity()` below catches drift before it ships.
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
  /** ISO 639-1/2 codes the model can decode. */
  languages: string[];
  /** True for the entry that is pre-selected on a fresh install. */
  default: boolean;
}

const PARAKEET_LANGUAGES = [
  "en",
  "it",
  "de",
  "fr",
  "es",
  "pt",
  "nl",
  "pl",
  "ru",
  "uk",
  "cs",
  "hr",
  "bg",
  "da",
  "el",
  "et",
  "fi",
  "hu",
  "lv",
  "lt",
  "mt",
  "ro",
  "sk",
  "sl",
  "sv",
];

export const STT_MODELS: SttModelEntry[] = [
  {
    id: "parakeet-tdt-0.6b-v3",
    displayName: "Parakeet TDT v3",
    // q4_k GGUF, ~644 MB on disk. Mirrors catalog.rs.
    sizeBytes: 644_000_000,
    languageCoverage: "european_25",
    summary: "25 European languages. Runs fully on-device.",
    license: "CC-BY-4.0",
    languages: PARAKEET_LANGUAGES,
    default: true,
  },
];

export function defaultModelId(): string {
  const def = STT_MODELS.find((m) => m.default);
  if (!def) {
    throw new Error("STT_MODELS catalog has no default entry");
  }
  return def.id;
}

export function findModel(id: string): SttModelEntry | undefined {
  return STT_MODELS.find((m) => m.id === id);
}

/** Catalog id of the STT model. */
export const PARAKEET_MODEL_ID = "parakeet-tdt-0.6b-v3";

/** On-disk filename of the fixed STT GGUF (mirrors stt::catalog::STT_GGUF_FILENAME). */
export const PARAKEET_FILENAME = "tdt-0.6b-v3-q4_k.gguf";

/** Catalog id of the fixed cleanup LLM (mirrors the single llm entry in
 * inference-core/data/model_catalog.json). */
export const CLEANUP_MODEL_ID = "gemma-3-1b-it-q4";

/** Display metadata for the fixed cleanup model. Kept in lockstep with the
 * backend catalog by `assertCatalogParity()` (dev-only). */
export const CLEANUP_MODEL = {
  id: CLEANUP_MODEL_ID,
  displayName: "Gemma 3 1B",
  sizeBytes: 806058272,
};

/**
 * Resolve the STT model for a given language code. With a single-model
 * catalog, always returns Parakeet regardless of the language.
 */
export function modelForLanguage(_code: string): string {
  return PARAKEET_MODEL_ID;
}

/**
 * ISO 639-1/2 display names for the wizard language picker. Kept inline:
 * the universe is the small union of Parakeet + curated Whisper, and a
 * runtime locale-display dependency would dwarf the strings themselves.
 */
const LANGUAGE_NAMES: Record<string, string> = {
  ar: "Arabic",
  bg: "Bulgarian",
  cs: "Czech",
  da: "Danish",
  de: "German",
  el: "Greek",
  en: "English",
  es: "Spanish",
  et: "Estonian",
  fa: "Persian",
  fi: "Finnish",
  fr: "French",
  he: "Hebrew",
  hi: "Hindi",
  hr: "Croatian",
  hu: "Hungarian",
  id: "Indonesian",
  it: "Italian",
  ja: "Japanese",
  ko: "Korean",
  lt: "Lithuanian",
  lv: "Latvian",
  ms: "Malay",
  mt: "Maltese",
  nl: "Dutch",
  no: "Norwegian",
  pl: "Polish",
  pt: "Portuguese",
  ro: "Romanian",
  ru: "Russian",
  sk: "Slovak",
  sl: "Slovenian",
  sv: "Swedish",
  sw: "Swahili",
  th: "Thai",
  tr: "Turkish",
  uk: "Ukrainian",
  vi: "Vietnamese",
  zh: "Chinese",
};

export function languageLabel(code: string): string {
  return LANGUAGE_NAMES[code] ?? code.toUpperCase();
}

export interface WizardLanguage {
  code: string;
  label: string;
}

/**
 * Resolve the language list to show in the wizard language step for a
 * given model. Returns the model's own language list.
 */
export function languagesForModel(id: string): string[] {
  const m = findModel(id);
  if (!m) return [];
  return [...m.languages];
}

/**
 * Languages offered in the wizard's language step: Parakeet's 25 European
 * languages, sorted by display name. Picking any of these resolves to
 * Parakeet via {@link modelForLanguage}.
 */
export const WIZARD_LANGUAGES: WizardLanguage[] = PARAKEET_LANGUAGES.map((code) => ({
  code,
  label: languageLabel(code),
})).sort((a, b) => a.label.localeCompare(b.label));

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
    backend = await invoke<BackendCatalogEntry[]>("get_stt_catalog");
  } catch (e) {
    // Tauri unavailable (e.g. running vitest under jsdom) — treat as no-op.
    // The Rust-side unit tests are the canonical guarantee.
    console.debug("[stt-catalog] backend probe failed, skipping parity check:", e);
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
      "displayName",
      "sizeBytes",
      "languageCoverage",
      "summary",
      "license",
      "default",
    ];
    for (const f of fields) {
      if (fe[f] !== be[f]) {
        throw new Error(
          `[stt-catalog] '${fe.id}'.${f} mismatch — frontend=${JSON.stringify(fe[f])}, backend=${JSON.stringify(be[f])}`,
        );
      }
    }
    if (
      fe.languages.length !== be.languages.length ||
      fe.languages.some((l, i) => l !== be.languages[i])
    ) {
      throw new Error(
        `[stt-catalog] '${fe.id}'.languages mismatch — frontend=${JSON.stringify(fe.languages)}, backend=${JSON.stringify(be.languages)}`,
      );
    }
  }

  // Guard the fixed cleanup model too (backend inference-core catalog).
  let llmCatalog: { id: string; kind: string; displayName: string; sizeBytes: number }[];
  try {
    llmCatalog = await invoke("models_catalog");
  } catch (e) {
    console.debug("[catalog] models_catalog probe failed, skipping LLM parity:", e);
    return;
  }
  const llms = llmCatalog.filter((c) => c.kind === "llm");
  if (llms.length !== 1) {
    throw new Error(`[catalog] expected exactly 1 shipped LLM, backend has ${llms.length}`);
  }
  const be = llms[0];
  if (
    be.id !== CLEANUP_MODEL.id ||
    be.displayName !== CLEANUP_MODEL.displayName ||
    be.sizeBytes !== CLEANUP_MODEL.sizeBytes
  ) {
    throw new Error(
      `[catalog] fixed LLM drift — frontend=${JSON.stringify(CLEANUP_MODEL)}, backend=${JSON.stringify(be)}`,
    );
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
