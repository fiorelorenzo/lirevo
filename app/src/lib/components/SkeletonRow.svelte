<script lang="ts">
  interface Props {
    class?: string;
  }
  let { class: cls = "h-4 w-full" }: Props = $props();
</script>

<div class="{cls} bg-muted rounded-md animate-shimmer"></div>
