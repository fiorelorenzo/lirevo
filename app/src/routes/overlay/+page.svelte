<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';

  // Bypass the shared `audioLevel` / `recording` stores: each webview window
  // gets its own module instance, and the shared store was emitting only its
  // initial value (0) into this window — the async Tauri listener inside
  // the store factory wasn't ever delivering events here. Listen directly
  // to the raw Tauri events instead, and log to the backend so we can
  // confirm registration from the lda log (overlay is click-through so
  // devtools isn't reachable here).
  const flog = (msg: string) => {
    void invoke('frontend_log', { source: 'overlay', msg }).catch(() => {});
  };

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

  let unlistenLevel: UnlistenFn | null = null;
  let unlistenRec: UnlistenFn | null = null;
  let lastRec = false;

  onMount(() => {
    void listen<number>('recording:level', (e) => {
      const level = e.payload;
      barsBuf = [...barsBuf.slice(1), level];
      bars = barsBuf.slice();
    })
      .then((u) => { unlistenLevel = u; })
      .catch((err) => { flog(`recording:level listen failed: ${err}`); });

    // Reset bars at the start of each fresh take so the carry-over from the
    // previous recording doesn't render as a peak the user didn't make.
    void listen<boolean>('recording:state', (e) => {
      const rec = e.payload;
      if (rec && !lastRec) {
        barsBuf = Array(BARS).fill(0);
        bars = barsBuf.slice();
      }
      lastRec = rec;
    })
      .then((u) => { unlistenRec = u; })
      .catch((err) => { flog(`recording:state listen failed: ${err}`); });
  });

  onDestroy(() => {
    unlistenLevel?.();
    unlistenRec?.();
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
