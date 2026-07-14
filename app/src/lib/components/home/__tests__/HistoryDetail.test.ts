/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import HistoryDetail from "../HistoryDetail.svelte";
import { settings } from "$lib/stores/settings.svelte";
import type { Dictation, Settings } from "$lib/tauri";

const BASE_SETTINGS = { styleLearningEnabled: true } as Settings;

const BASE_DICTATION: Dictation = {
  id: 1,
  createdAt: Date.now(),
  language: "en",
  sttModel: "parakeet-v3",
  audioMs: 3000,
  rawText: "raw transcript",
  sttMs: 100,
  llmModel: "gemma-3-1b",
  cleanedText: "Cleaned transcript.",
  cleanMs: 200,
  cleanupStatus: "applied",
  cleanupError: null,
  injectMethod: "pasteboard",
  injectMs: 50,
  totalMs: 350,
  targetApp: "Mail",
  targetBundle: "com.apple.mail",
  inputDevice: null,
  smartRoutingEnabled: null,
  smartRoutingApplied: null,
};

function mockHistoryGet(dictation: Dictation | null): void {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "history_get") return Promise.resolve(dictation);
    if (cmd === "style_example_pin") return Promise.resolve(undefined);
    return Promise.resolve(undefined);
  });
}

const PIN_LABEL = "Save as style example";

describe("HistoryDetail pin action", () => {
  beforeEach(() => {
    settings.set(BASE_SETTINGS);
    vi.mocked(invoke).mockReset();
  });

  it("shows the pin button when style learning is on, there's a target app, and cleanup ran", async () => {
    mockHistoryGet(BASE_DICTATION);
    render(HistoryDetail, { id: 1 });

    expect(await screen.findByText(PIN_LABEL)).toBeInTheDocument();
  });

  it("hides the pin button when style_learning_enabled is off", async () => {
    settings.set({ ...BASE_SETTINGS, styleLearningEnabled: false });
    mockHistoryGet(BASE_DICTATION);
    render(HistoryDetail, { id: 1 });

    await waitFor(() => expect(screen.getByText("Cleaned transcript.")).toBeInTheDocument());
    expect(screen.queryByText(PIN_LABEL)).not.toBeInTheDocument();
  });

  it("hides the pin button when the dictation has no target_bundle", async () => {
    mockHistoryGet({ ...BASE_DICTATION, targetBundle: null });
    render(HistoryDetail, { id: 1 });

    await waitFor(() => expect(screen.getByText("Cleaned transcript.")).toBeInTheDocument());
    expect(screen.queryByText(PIN_LABEL)).not.toBeInTheDocument();
  });

  it("hides the pin button when cleanup was skipped", async () => {
    mockHistoryGet({
      ...BASE_DICTATION,
      cleanupStatus: "skipped",
      llmModel: null,
      cleanMs: null,
    });
    render(HistoryDetail, { id: 1 });

    await waitFor(() =>
      expect(screen.getByText("Skipped (dictation-only)")).toBeInTheDocument(),
    );
    expect(screen.queryByText(PIN_LABEL)).not.toBeInTheDocument();
  });

  it("pins the dictation and disables the button on success", async () => {
    mockHistoryGet(BASE_DICTATION);
    render(HistoryDetail, { id: 1 });

    const button = await screen.findByText(PIN_LABEL);
    await fireEvent.click(button);

    expect(invoke).toHaveBeenCalledWith("style_example_pin", { dictationId: 1 });
    await waitFor(() => expect(screen.getByText("Saved as style example")).toBeInTheDocument());
    expect(screen.getByRole("button")).toBeDisabled();
  });
});
