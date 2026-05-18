<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { audioLevel, recording } from '$lib/stores/recording';

  // The window itself is shown/hidden by the backend (hotkey.rs) on
  // recording start/stop. This component is just the renderer — subscribe
  // to live RMS levels and reset the buffer when a fresh take starts.

  // Number of vertical bars in the waveform. Each shift represents one
  // audio-level sample (≈30 Hz so the bar sweep at this resolution moves at
  // a perceptual "slow waveform" pace, not "glitchy strobe").
  const BARS = 36;
  // Visual exaggeration: input RMS rarely tops 0.3 even during clear
  // speech, so a raw level→pixel scale gives barely-perceptible bars.
  // Apply gamma (sqrt) to lift quiet speech off the floor + multiply.
  const MAX_BAR_HEIGHT = 44; // px, half-height for symmetric draw
  function shape(level: number): number {
    // sqrt curve: 0.05 → 0.22, 0.2 → 0.45, 0.5 → 0.71
    const eased = Math.sqrt(Math.max(0, Math.min(1, level)));
    return Math.max(3, eased * 1.6 * MAX_BAR_HEIGHT);
  }

  let bars = $state<number[]>(Array(BARS).fill(0));
  // Mutated imperatively (not $state) — the audioLevel subscribe can fire
  // ~30 Hz and we don't want Svelte to rebuild a reactive proxy each tick.
  // After mutating we copy the snapshot into `bars`.
  let barsBuf: number[] = Array(BARS).fill(0);

  let unsubLevel: (() => void) | null = null;
  let unsubRec: (() => void) | null = null;
  let lastRec = false;
  // TEMP debug counter: number of audioLevel events received in this window
  // since mount. Rendered as text so we can tell from the overlay itself
  // whether the subscription works without opening devtools (overlay is
  // click-through so right-click → Inspect isn't reachable).
  let levelCount = $state(0);
  let lastLevel = $state(0);

  onMount(() => {
    unsubLevel = audioLevel.subscribe((level) => {
      levelCount += 1;
      lastLevel = level;
      barsBuf = [...barsBuf.slice(1), level];
      bars = barsBuf.slice();
    });
    // Reset bars at the start of each fresh take so the carry-over from the
    // previous recording doesn't render as a peak the user didn't make.
    unsubRec = recording.subscribe((rec) => {
      if (rec && !lastRec) {
        barsBuf = Array(BARS).fill(0);
        bars = barsBuf.slice();
      }
      lastRec = rec;
    });
  });

  onDestroy(() => {
    unsubLevel?.();
    unsubRec?.();
  });
</script>

<svelte:head>
  <!--
    Make the webview itself transparent. Tauri's transparent window relies
    on the HTML root having no opaque background — otherwise the rounded
    pill bleeds a black square around itself.
  -->
  <style>
    html, body, #svelte {
      background: transparent !important;
      margin: 0;
      overflow: hidden;
      height: 100%;
    }
  </style>
</svelte:head>

<div class="overlay-root">
  <div class="pill">
    <span class="rec-dot">
      <span class="rec-dot-ping"></span>
      <span class="rec-dot-core"></span>
    </span>

    <div class="waveform">
      {#each bars as level, i (i)}
        <div class="bar" style="height: {shape(level)}px"></div>
      {/each}
    </div>
    <!-- TEMP debug — remove once we confirm subscribe works -->
    <span class="debug">{levelCount} / {lastLevel.toFixed(2)}</span>
  </div>
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

  .pill {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    border-radius: 9999px;
    background: rgba(15, 17, 21, 0.88);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45),
      0 1px 0 rgba(255, 255, 255, 0.06) inset;
  }

  .rec-dot {
    position: relative;
    display: inline-flex;
    width: 8px;
    height: 8px;
  }
  .rec-dot-ping {
    position: absolute;
    inset: 0;
    border-radius: 9999px;
    background: #ef4444;
    opacity: 0.55;
    animation: ping 1.4s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  .rec-dot-core {
    position: relative;
    width: 8px;
    height: 8px;
    border-radius: 9999px;
    background: #ef4444;
  }
  @keyframes ping {
    0%   { transform: scale(1);    opacity: 0.55; }
    75%  { transform: scale(2.2);  opacity: 0; }
    100% { transform: scale(2.2);  opacity: 0; }
  }

  .waveform {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 44px;
    width: 188px;
  }
  .debug {
    font-family: ui-monospace, monospace;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.55);
    white-space: nowrap;
  }

  .bar {
    width: 3px;
    border-radius: 9999px;
    /* Gradient — brighter in the middle, fades to translucent at the
       tips. Gives the bars a glowing, lively look when they extend. */
    background: linear-gradient(
      to bottom,
      rgba(110, 168, 254, 0.85) 0%,
      rgba(180, 220, 255, 1) 50%,
      rgba(110, 168, 254, 0.85) 100%
    );
    box-shadow: 0 0 6px rgba(140, 200, 255, 0.45);
    transition: height 90ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
</style>
