import { describe, it, expect } from "vitest";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";
import { CLEANUP_MODEL, PARAKEET_FILENAME } from "$lib/models/catalog";

// Repo root: app/src/lib/models/__tests__/ -> app/src/lib/models -> app/src/lib
// -> app/src -> app -> <repo root>. Resolved from this file's own path (not
// process.cwd()) so the test passes whether `pnpm test` runs from the repo
// root or from `app/`.
//
// `fileURLToPath` is called with the raw `import.meta.url` string rather
// than a `new URL(...)` instance: under the jsdom test environment the
// global `URL` constructor is jsdom's polyfill, not Node's, and Node's
// `fileURLToPath` rejects a non-Node URL instance.
const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../../../..");

describe("catalog parity with backend", () => {
  it("mirrors the single shipped LLM entry in inference-core's model_catalog.json", () => {
    const catalogPath = path.join(REPO_ROOT, "crates/inference-core/data/model_catalog.json");
    const raw = fs.readFileSync(catalogPath, "utf-8");
    const catalog = JSON.parse(raw) as {
      llm: { id: string; displayName: string; sizeBytes: number }[];
    };

    expect(catalog.llm.length).toBe(1);
    const backendLlm = catalog.llm[0];
    expect(backendLlm.id).toBe(CLEANUP_MODEL.id);
    expect(backendLlm.displayName).toBe(CLEANUP_MODEL.displayName);
    expect(backendLlm.sizeBytes).toBe(CLEANUP_MODEL.sizeBytes);
  });

  it("mirrors the STT GGUF filename in stt/catalog.rs", () => {
    const catalogRsPath = path.join(REPO_ROOT, "app/src-tauri/src/stt/catalog.rs");
    const raw = fs.readFileSync(catalogRsPath, "utf-8");
    const match = raw.match(/STT_GGUF_FILENAME:\s*&str\s*=\s*"([^"]+)"/);
    expect(match, "STT_GGUF_FILENAME const not found in stt/catalog.rs").not.toBeNull();
    expect(match![1]).toBe(PARAKEET_FILENAME);
  });
});
