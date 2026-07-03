<script lang="ts">
  interface Props {
    size?: number;
  }
  let { size = 24 }: Props = $props();
</script>

<svg
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="3"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d="M4 12l5 5L20 6" pathLength="1" class="check-path" />
</svg>

<style>
  .check-path {
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: draw 600ms cubic-bezier(0.19, 1, 0.22, 1) forwards;
  }
  @keyframes draw {
    to {
      stroke-dashoffset: 0;
    }
  }
</style>
