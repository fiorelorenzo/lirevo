<script lang="ts">
  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();
</script>
<div class="h-full flex items-center justify-center">
  <button onclick={onnext} class="px-4 py-2 rounded-md bg-primary text-primary-foreground">
    Models (T35) — Next
  </button>
</div>
