import { describe, it, expect } from "vitest";
import { modelInstallState } from "../model-status";
import type { LocalModel } from "$lib/tauri";

const local: LocalModel[] = [
  {
    id: "custom:tdt-0.6b-v3-q4_k.gguf",
    kind: "stt",
    path: "/m/tdt-0.6b-v3-q4_k.gguf",
    sizeBytes: 644,
    inCatalog: false,
  },
  {
    id: "gemma-3-1b-it-q4",
    kind: "llm",
    path: "/m/gemma-3-1b-it-Q4_K_M.gguf",
    sizeBytes: 806,
    inCatalog: true,
  },
];

describe("modelInstallState", () => {
  it("matches STT by filename suffix", () => {
    expect(modelInstallState(local, { filename: "tdt-0.6b-v3-q4_k.gguf" })).toEqual({
      installed: true,
      sizeBytes: 644,
    });
  });
  it("matches LLM by catalog id", () => {
    expect(modelInstallState(local, { id: "gemma-3-1b-it-q4" })).toEqual({
      installed: true,
      sizeBytes: 806,
    });
  });
  it("reports missing models", () => {
    expect(modelInstallState([], { id: "gemma-3-1b-it-q4" })).toEqual({
      installed: false,
      sizeBytes: null,
    });
  });
});
