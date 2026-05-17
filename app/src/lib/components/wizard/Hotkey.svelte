<script lang="ts">
  interface Props { onfinish: () => void; }
  let { onfinish }: Props = $props();
</script>
<div class="h-full flex items-center justify-center">
  <button onclick={onfinish} class="px-4 py-2 rounded-md bg-primary text-primary-foreground">
    Hotkey (T35) — Finish
  </button>
</div>
