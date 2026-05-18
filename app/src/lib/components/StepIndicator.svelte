<script lang="ts">
  interface Props { step: number; total: number; }
  let { step, total }: Props = $props();
</script>

<div class="flex flex-col gap-2">
  <div class="flex items-center gap-1.5">
    {#each Array(total) as _, i (i)}
      <div
        class="h-1 flex-1 rounded-full transition-all duration-[400ms]"
        style="background: {i <= step ? 'var(--color-primary)' : 'var(--color-muted)'}; opacity: {i === step ? 1 : i < step ? 0.7 : 1};"
      ></div>
    {/each}
  </div>
  <div class="text-xs text-muted-foreground tabular-nums">
    Step {step + 1} of {total}
  </div>
</div>
