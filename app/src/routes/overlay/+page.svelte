<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { audioLevel, recording } from '$lib/stores/recording';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  // Number of vertical bars in the waveform. Each shift represents one
  // audio-level sample (≈30 Hz so the bar sweep at this resolution moves at
  // a perceptual `slow waveform` pace, not `glitchy strobe`).
  const BARS = 28;
  let bars = $state<number[]>(Array(BARS).fill(0));
  // Mutated imperatively (not $state) — the audioLevel subscribe can fire
  // ~30 Hz and we don't want Svelte to rebuild a reactive proxy each tick.
  // After mutating we copy the snapshot into `bars`.
  let barsBuf: number[] = Array(BARS).fill(0);

  let unsubLevel: (() => void) | null = null;
  let unsubRec: (() => void) | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  let lastRec = false;

  onMount(() => {
    const win = getCurrentWindow();

    unsubLevel = audioLevel.subscribe((level) => {
      barsBuf = [...barsBuf.slice(1), level];
      bars = barsBuf.slice();
    });

    unsubRec = recording.subscribe((rec) => {
      if (rec === lastRec) return;
      lastRec = rec;
      if (rec) {
        if (hideTimer) {
          clearTimeout(hideTimer);
          hideTimer = null;
        }
        // Reset bars when a fresh take starts so we don't show the tail
        // of the previous recording at peak height.
        barsBuf = Array(BARS).fill(0);
        bars = barsBuf.slice();
        void win.show();
      } else {
        // Brief delay so the user sees the bars come to rest before the
        // overlay disappears.
        hideTimer = setTimeout(() => { void win.hide(); }, 600);
      }
    });
  });

  onDestroy(() => {
    unsubLevel?.();
    unsubRec?.();
    if (hideTimer) clearTimeout(hideTimer);
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
        <div
          class="bar"
          style="height: {Math.max(2, Math.min(36, level * 80))}px"
        ></div>
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
    gap: 3px;
    height: 36px;
  }
  .bar {
    width: 2px;
    border-radius: 9999px;
    background: rgba(255, 255, 255, 0.92);
    transition: height 70ms linear;
  }
</style>
