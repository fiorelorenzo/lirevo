<script lang="ts">
  interface Props { title?: string; }
  let { title = '' }: Props = $props();
</script>

<div
  data-tauri-drag-region
  class="h-12 w-full flex items-center justify-center select-none border-b border-border/50 bg-surface/60 backdrop-blur-md"
>
  <div class="text-xs font-medium text-muted-foreground tracking-wide">
    {title}
  </div>
</div>
