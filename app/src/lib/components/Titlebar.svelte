<script lang="ts">
  interface Props {
    title?: string;
  }
  let { title = "" }: Props = $props();
</script>

<div
  data-tauri-drag-region
  class="h-12 w-full flex items-center justify-center select-none border-b border-border/50 bg-surface/60 backdrop-blur-md"
>
  <!-- Inner element is presentational only — let clicks fall through to the
       drag-region parent so the whole titlebar is draggable. -->
  <div class="text-xs font-medium text-muted-foreground tracking-wide pointer-events-none">
    {title}
  </div>
</div>
