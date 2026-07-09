<script lang="ts">
  import { lda } from "$lib/tauri";
  import {
    formatHotkey,
    validateHotkey,
    type HotkeySpec,
    type ActivationMode,
    type Os,
    type CaptureEvent,
  } from "$lib/hotkey";
  import { initialCaptureState, stepCapture, type CaptureState } from "$lib/hotkey-capture";
  import { Button } from "$lib/components/ui/button";
  import { onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    spec: HotkeySpec;
    mode: ActivationMode;
    os?: Os;
    onchange: (next: { hotkey: HotkeySpec; activationMode: ActivationMode }) => void;
  }
  let { spec, mode, os = "macos", onchange }: Props = $props();

  let capturing = $state(false);
  let error = $state<string | null>(null);
  let liveChips = $state<string[]>([]);
  let unlisten: UnlistenFn | null = null;
  let capture: CaptureState = initialCaptureState();

  const chips = $derived(formatHotkey(spec, os));

  function finalize(next: HotkeySpec) {
    const v = validateHotkey(next, os);
    if (!v.ok) {
      // Keep listening so the user can correct without re-clicking Change.
      error = v.error ?? "Invalid shortcut";
      return;
    }
    error = null;
    onchange({ hotkey: next, activationMode: mode });
    void stop();
  }

  async function start() {
    error = null;
    liveChips = [];
    capture = initialCaptureState();
    capturing = true;
    unlisten = await lda.onHotkeyCapture(handleCapture);
    await lda.startHotkeyCapture();
  }

  async function stop() {
    capturing = false;
    capture = initialCaptureState();
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    await lda.stopHotkeyCapture();
  }

  function handleCapture(e: CaptureEvent) {
    // Live preview of the chord currently held.
    if (e.baseKey) {
      liveChips = formatHotkey({ modifiers: e.modifiers, trigger: { key: e.baseKey } }, os);
    } else {
      liveChips = formatHotkey({ modifiers: e.modifiers, trigger: { key: "" } }, os).filter(
        (c) => c !== "",
      );
    }
    // Commit only once every key is released, so multi-key combos can build up
    // instead of the first key winning instantly.
    const r = stepCapture(capture, e);
    capture = r.state;
    if (r.commit) finalize(r.commit);
  }

  function onKeydown(ev: KeyboardEvent) {
    if (capturing && ev.key === "Escape") {
      ev.preventDefault();
      void stop();
    }
  }

  onDestroy(() => {
    if (capturing) void stop();
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="space-y-3">
  <div class="flex items-center gap-3">
    <div
      class={[
        "flex min-h-10 flex-1 flex-wrap items-center gap-1.5 rounded-lg border bg-surface px-3 py-2 transition-colors",
        capturing ? "border-primary ring-2 ring-primary/30" : "border-border",
      ].join(" ")}
    >
      {#if capturing}
        <span class="hotkey-pulse text-sm text-muted-foreground"
          >Listening… press your shortcut</span
        >
        {#each liveChips as c (c)}
          <kbd class="rounded bg-accent px-1.5 py-0.5 font-mono text-xs">{c}</kbd>
        {/each}
      {:else}
        {#each chips as c (c)}
          <kbd class="rounded bg-accent px-1.5 py-0.5 font-mono text-xs">{c}</kbd>
        {/each}
      {/if}
    </div>
    <Button variant="outline" size="sm" onclick={() => (capturing ? void stop() : void start())}>
      {capturing ? "Cancel" : "Change"}
    </Button>
  </div>

  {#if error}
    <p class="text-xs text-destructive">{error}</p>
  {/if}

  <div class="inline-flex overflow-hidden rounded-lg border border-border text-sm">
    <button
      type="button"
      aria-pressed={mode === "hold"}
      class={[
        "px-3 py-1.5 transition-colors",
        mode === "hold" ? "bg-primary/10 text-primary" : "hover:bg-accent",
      ].join(" ")}
      onclick={() => onchange({ hotkey: spec, activationMode: "hold" })}
    >
      Hold
    </button>
    <button
      type="button"
      aria-pressed={mode === "tap"}
      class={[
        "border-l border-border px-3 py-1.5 transition-colors",
        mode === "tap" ? "bg-primary/10 text-primary" : "hover:bg-accent",
      ].join(" ")}
      onclick={() => onchange({ hotkey: spec, activationMode: "tap" })}
    >
      Tap
    </button>
  </div>
  <p class="text-xs text-muted-foreground">
    {mode === "hold"
      ? "Hold to talk, release to transcribe."
      : "Press once to start, again to stop."}
  </p>
</div>

<style>
  .hotkey-pulse {
    animation: hotkey-fade 1.4s var(--ease-in-out-soft, ease-in-out) infinite;
  }

  @keyframes hotkey-fade {
    0%,
    100% {
      opacity: 0.5;
    }
    50% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .hotkey-pulse {
      animation: none;
    }
  }
</style>
