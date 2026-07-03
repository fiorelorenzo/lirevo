<script lang="ts">
  import { MessagesSquare } from "@lucide/svelte";
  import type { ActivationMode } from "$lib/hotkey";

  interface Props {
    /** Chip tokens for the configured hotkey (e.g. ["⌥ right"] or ["⌃","⇧","K"]). */
    hotkeyChips?: string[];
    /** Activation mode, so the prompt verb matches Tap/Hold. */
    mode?: ActivationMode;
  }
  let { hotkeyChips, mode = "hold" }: Props = $props();
</script>

<div class="flex-1 flex flex-col items-center justify-center gap-5 px-8 text-center">
  <div class="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center">
    <MessagesSquare class="w-8 h-8 text-primary" />
  </div>
  <div class="space-y-1.5">
    <h2 class="text-lg font-semibold">Your dictations will appear here</h2>
    <p class="text-sm text-muted-foreground max-w-xs">
      Every transcription and cleanup you run is saved locally so you can review it later.
    </p>
  </div>
  {#if hotkeyChips && hotkeyChips.length}
    <div class="flex flex-col items-center gap-2">
      <div class="flex flex-wrap items-center justify-center gap-1">
        {#each hotkeyChips as c (c)}
          <kbd class="rounded bg-accent px-1.5 py-0.5 font-mono text-xs">{c}</kbd>
        {/each}
      </div>
      <p class="text-xs text-muted-foreground">
        {mode === "tap" ? "Tap to start dictating" : "Press and hold to start dictating"}
      </p>
    </div>
  {/if}
</div>
