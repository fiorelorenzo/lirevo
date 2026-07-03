<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "$lib/i18n";
  import type { PartialTranscript } from "$lib/tauri";

  // Overlay is click-through so devtools isn't reachable here — log to the
  // backend instead so listener registration can be confirmed from the log.
  const flog = (msg: string) => {
    void invoke("frontend_log", { source: "overlay", msg }).catch(() => {});
  };

  // Number of vertical bars in the waveform.
  const BARS = 36;
  const MAX_BAR_HEIGHT = 44; // px, half-height for symmetric draw
  function shape(level: number): number {
    // sqrt curve lifts quiet speech off the floor.
    const eased = Math.sqrt(Math.max(0, Math.min(1, level)));
    return Math.max(3, eased * 1.6 * MAX_BAR_HEIGHT);
  }

  const LIVE_TEXT_MAX_CHARS = 120;

  // The overlay's lifecycle is driven by the backend `overlay:phase` event:
  //   recording  → live mic waveform (audio-reactive bars + red dot)
  //   processing → STT/cleanup running: a flowing "thinking" wave + blue dot.
  //                The overlay stays up the whole time (it does NOT vanish
  //                during cleanup).
  //   done       → the final text was handled; play the exit animation, then
  //                this webview hides its own window.
  type Phase = "idle" | "recording" | "processing" | "done";
  let phase = $state<Phase>("idle");
  let isRecording = $derived(phase === "recording");

  let bars = $state<number[]>(Array(BARS).fill(0));
  // Mutated imperatively (the level event fires ~30 Hz); snapshot into `bars`.
  let barsBuf: number[] = Array(BARS).fill(0);

  let partialText = $state("");
  let partialFinal = $state(false);

  let displayText = $derived.by(() => {
    const s = partialText;
    if (s.length <= LIVE_TEXT_MAX_CHARS) return s;
    return "…" + s.slice(s.length - LIVE_TEXT_MAX_CHARS + 1);
  });
  // The text pill shows while recording (live transcript / placeholder) or
  // whenever there is captured text to keep visible through processing.
  let showText = $derived(isRecording || partialText.length > 0);

  let unlistenLevel: UnlistenFn | null = null;
  let unlistenPartial: UnlistenFn | null = null;
  let unlistenPhase: UnlistenFn | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  // Keep in sync with the `.leaving` exit animation duration below.
  const EXIT_MS = 420;

  function cancelHide() {
    if (hideTimer !== null) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
  }
  function resetTake() {
    barsBuf = Array(BARS).fill(0);
    bars = barsBuf.slice();
    partialText = "";
    partialFinal = false;
  }

  onMount(() => {
    void listen<number>("recording:level", (e) => {
      barsBuf = [...barsBuf.slice(1), e.payload];
      bars = barsBuf.slice();
    })
      .then((u) => {
        unlistenLevel = u;
      })
      .catch((err) => flog(`recording:level listen failed: ${err}`));

    void listen<PartialTranscript>("recording:partial_transcript", (e) => {
      partialText = e.payload.text;
      partialFinal = e.payload.isFinal;
    })
      .then((u) => {
        unlistenPartial = u;
      })
      .catch((err) => flog(`recording:partial_transcript listen failed: ${err}`));

    void listen<string>("overlay:phase", (e) => {
      const p = e.payload as Phase;
      if (p === "recording") {
        cancelHide();
        resetTake();
        phase = "recording";
      } else if (p === "processing") {
        cancelHide();
        phase = "processing";
      } else if (p === "done") {
        cancelHide();
        phase = "done";
        // Let the exit animation play, then hide our own window and reset so
        // the next take starts clean.
        hideTimer = setTimeout(() => {
          void getCurrentWindow()
            .hide()
            .catch(() => {});
          phase = "idle";
          resetTake();
          hideTimer = null;
        }, EXIT_MS);
      }
    })
      .then((u) => {
        unlistenPhase = u;
      })
      .catch((err) => flog(`overlay:phase listen failed: ${err}`));
  });

  onDestroy(() => {
    unlistenLevel?.();
    unlistenPartial?.();
    unlistenPhase?.();
    cancelHide();
  });
</script>

<svelte:head>
  <!--
    Make the webview itself transparent. Tauri's transparent window relies
    on the HTML root having no opaque background — otherwise the rounded
    pill bleeds a black square around itself.
  -->
  <style>
    html,
    body,
    #svelte {
      background: transparent !important;
      margin: 0;
      overflow: hidden;
      height: 100%;
    }
  </style>
</svelte:head>

<div class="overlay-root">
  {#if phase !== "idle"}
    <div class="pill-stack" class:leaving={phase === "done"}>
      <div class="pill">
        <span class="dot" class:recording={isRecording} class:processing={!isRecording}>
          {#if isRecording}
            <span class="dot-ping"></span>
          {/if}
          <span class="dot-core"></span>
        </span>

        <div class="waveform" class:processing={!isRecording}>
          {#each bars as level, i (i)}
            <div
              class="bar"
              style:--i={i}
              style:height={isRecording ? `${shape(level)}px` : null}
            ></div>
          {/each}
        </div>
      </div>

      {#if showText}
        <div class="live-text" class:final={partialFinal} class:processing={!isRecording}>
          {#if displayText.length === 0}
            <span class="placeholder">{t("overlay.live_transcript_placeholder")}</span>
          {:else}
            <span class="live-text-body">{displayText}</span>
          {/if}
          {#if isRecording && !partialFinal}
            <span class="caret" aria-hidden="true"></span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .overlay-root {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 8px;
  }

  .pill-stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    animation: pop-in 240ms cubic-bezier(0.2, 0.9, 0.2, 1);
  }
  .pill-stack.leaving {
    animation: pop-out 420ms cubic-bezier(0.4, 0, 0.2, 1) forwards;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(-6px) scale(0.96);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @keyframes pop-out {
    to {
      opacity: 0;
      transform: translateY(-6px) scale(0.96);
    }
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    border-radius: 9999px;
    background: rgba(15, 17, 21, 0.88);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.45),
      0 1px 0 rgba(255, 255, 255, 0.06) inset;
  }

  .live-text {
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 360px;
    padding: 6px 14px;
    border-radius: 9999px;
    background: rgba(15, 17, 21, 0.72);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    color: rgba(220, 230, 245, 0.92);
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 13px;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    transition: opacity 220ms ease;
  }
  .live-text.final {
    color: rgba(200, 215, 235, 0.78);
  }
  .live-text-body {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* During processing the captured text shimmers to signal "polishing". */
  .live-text.processing .live-text-body {
    background: linear-gradient(
      90deg,
      rgba(150, 180, 220, 0.55) 0%,
      rgba(235, 244, 255, 0.95) 50%,
      rgba(150, 180, 220, 0.55) 100%
    );
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    animation: shimmer 1.6s linear infinite;
  }
  @keyframes shimmer {
    from {
      background-position: 150% 0;
    }
    to {
      background-position: -150% 0;
    }
  }
  .placeholder {
    color: rgba(180, 195, 215, 0.55);
    font-style: italic;
  }
  .caret {
    display: inline-block;
    width: 6px;
    height: 14px;
    margin-left: 4px;
    background: rgba(180, 220, 255, 0.85);
    border-radius: 1px;
    animation: caret-blink 1.05s steps(2, end) infinite;
  }
  @keyframes caret-blink {
    0%,
    50% {
      opacity: 1;
    }
    50.01%,
    100% {
      opacity: 0;
    }
  }

  .dot {
    position: relative;
    display: inline-flex;
    width: 8px;
    height: 8px;
  }
  .dot-core {
    position: relative;
    width: 8px;
    height: 8px;
    border-radius: 9999px;
  }
  .dot.recording .dot-core {
    background: #ef4444;
  }
  .dot.processing .dot-core {
    background: #6ea8fe;
    transform-origin: center;
    animation: dot-breathe 1.1s ease-in-out infinite;
  }
  .dot-ping {
    position: absolute;
    inset: 0;
    border-radius: 9999px;
    background: #ef4444;
    opacity: 0.55;
    animation: ping 1.4s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  @keyframes ping {
    0% {
      transform: scale(1);
      opacity: 0.55;
    }
    75% {
      transform: scale(2.2);
      opacity: 0;
    }
    100% {
      transform: scale(2.2);
      opacity: 0;
    }
  }
  @keyframes dot-breathe {
    0%,
    100% {
      transform: scale(0.85);
      opacity: 0.7;
    }
    50% {
      transform: scale(1.25);
      opacity: 1;
    }
  }

  .waveform {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 44px;
    width: 188px;
  }
  .bar {
    width: 3px;
    border-radius: 9999px;
    background: linear-gradient(
      to bottom,
      rgba(110, 168, 254, 0.85) 0%,
      rgba(180, 220, 255, 1) 50%,
      rgba(110, 168, 254, 0.85) 100%
    );
    box-shadow: 0 0 6px rgba(140, 200, 255, 0.45);
    transition: height 90ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  /* Processing: a smooth wave travels across the bars (indeterminate),
     independent of any audio input. */
  .waveform.processing .bar {
    height: 14px;
    transform-origin: center center;
    animation: think 1.05s ease-in-out infinite;
    animation-delay: calc(var(--i) * -28ms);
  }
  @keyframes think {
    0%,
    100% {
      transform: scaleY(0.35);
      opacity: 0.5;
    }
    50% {
      transform: scaleY(2.4);
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pill-stack,
    .pill-stack.leaving,
    .dot.processing .dot-core,
    .dot-ping,
    .waveform.processing .bar,
    .live-text.processing .live-text-body,
    .caret {
      animation-duration: 0.001ms;
      animation-iteration-count: 1;
    }
    .pill-stack.leaving {
      opacity: 0;
    }
  }
</style>
