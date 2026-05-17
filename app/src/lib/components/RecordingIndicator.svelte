<script lang="ts">
  import { recording, audioLevel } from '$lib/stores/recording';
  import { fly } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';

  const BARS = 24;
  let bars = $state<number[]>(Array(BARS).fill(0));

  $effect(() => {
    if ($audioLevel != null && $recording) {
      bars = [...bars.slice(1), $audioLevel];
    }
  });

  $effect(() => {
    if (!$recording) {
      bars = Array(BARS).fill(0);
    }
  });
</script>

{#if $recording}
  <div
    transition:fly={{ y: 24, duration: 250, easing: quintOut }}
    class="fixed bottom-8 left-1/2 -translate-x-1/2 z-50 px-5 py-3 bg-surface-elevated/95 backdrop-blur-2xl border border-border/50 rounded-full shadow-xl flex items-center gap-3"
  >
    <span class="relative flex h-2.5 w-2.5">
      <span class="absolute inset-0 rounded-full bg-destructive animate-ping opacity-75"></span>
      <span class="relative inline-flex h-2.5 w-2.5 rounded-full bg-destructive"></span>
    </span>
    <div class="flex items-end gap-[2px] h-6">
      {#each bars as level, i (i)}
        <div
          class="w-[3px] bg-primary rounded-full transition-all duration-75"
          style="height: {Math.max(4, level * 24)}px"
        ></div>
      {/each}
    </div>
    <span class="text-xs font-medium text-muted-foreground tabular-nums">Recording</span>
  </div>
{/if}
