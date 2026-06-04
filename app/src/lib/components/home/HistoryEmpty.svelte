<script lang="ts">
  import { MessagesSquare } from '@lucide/svelte';
  import KeyChip from '$lib/components/KeyChip.svelte';

  interface Props {
    /** Glyph for the configured hotkey (e.g. "⌥"), or undefined when unknown. */
    hotkeyGlyph?: string;
    /** Human label under the glyph (e.g. "right", "Globe"). */
    hotkeyLabel?: string;
  }
  let { hotkeyGlyph, hotkeyLabel = '' }: Props = $props();
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
  {#if hotkeyGlyph}
    <div class="flex flex-col items-center gap-2">
      <KeyChip label={hotkeyLabel} glyph={hotkeyGlyph} size="sm" selected />
      <p class="text-xs text-muted-foreground">Press and hold to start dictating</p>
    </div>
  {/if}
</div>
