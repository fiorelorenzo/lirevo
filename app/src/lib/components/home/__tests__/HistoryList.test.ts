/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import HistoryList from "../HistoryList.svelte";
import HistoryEmpty from "../HistoryEmpty.svelte";
import type { DictationSummary } from "$lib/tauri";

const FIXTURES: DictationSummary[] = [
  {
    id: 1,
    createdAt: Date.now() - 5 * 60_000,
    preview: "First dictation preview",
    sttModel: "parakeet-v3",
    llmModel: "gemma-3-1b",
    targetApp: "Mail",
    totalMs: 1042,
    cleanupStatus: "applied",
  },
  {
    id: 2,
    createdAt: Date.now() - 2 * 3_600_000,
    preview: "Second dictation preview",
    sttModel: "whisper-large",
    llmModel: null,
    targetApp: null,
    totalMs: 880,
    cleanupStatus: "skipped",
  },
];

describe("HistoryList", () => {
  it("renders both previews and the model/target badges", () => {
    render(HistoryList, {
      items: FIXTURES,
      selectedId: null,
      onSelect: () => {},
      onDelete: () => {},
    });

    expect(screen.getByText("First dictation preview")).toBeInTheDocument();
    expect(screen.getByText("Second dictation preview")).toBeInTheDocument();
    // STT model badges
    expect(screen.getByText("parakeet-v3")).toBeInTheDocument();
    expect(screen.getByText("whisper-large")).toBeInTheDocument();
    // LLM badge present, and `raw` fallback when llmModel is null
    expect(screen.getByText("gemma-3-1b")).toBeInTheDocument();
    expect(screen.getByText("raw")).toBeInTheDocument();
    // target app shown only when present
    expect(screen.getByText("Mail")).toBeInTheDocument();
  });

  it("fires onSelect with the row id when a row is clicked", async () => {
    const onSelect = vi.fn();
    render(HistoryList, {
      items: FIXTURES,
      selectedId: null,
      onSelect,
      onDelete: () => {},
    });

    await fireEvent.click(screen.getByText("Second dictation preview"));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith(2);
  });
});

describe("HistoryEmpty", () => {
  it("renders the empty-state copy", () => {
    render(HistoryEmpty, {});
    expect(screen.getByText("Your dictations will appear here")).toBeInTheDocument();
  });
});
