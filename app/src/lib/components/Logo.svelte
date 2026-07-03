<script lang="ts">
  interface Props {
    size?: number;
    class?: string;
    loading?: boolean;
  }
  let { size = 64, class: cls = "", loading = false }: Props = $props();
</script>

<svg
  viewBox="0 0 64 64"
  width={size}
  height={size}
  class={[cls, loading ? "lirevo-logo-loading" : ""].join(" ")}
  aria-hidden="true"
>
  <defs>
    <linearGradient id="lirevo-logo-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="var(--color-primary)" />
      <stop offset="100%" stop-color="var(--color-accent-violet)" />
    </linearGradient>
  </defs>
  <g fill="url(#lirevo-logo-grad)">
    <rect x="10" y="28" width="4" height="8" rx="2" style:--bar-i="0" />
    <rect x="18" y="22" width="4" height="20" rx="2" style:--bar-i="1" />
    <rect x="26" y="16" width="4" height="32" rx="2" style:--bar-i="2" />
    <rect x="34" y="20" width="4" height="24" rx="2" style:--bar-i="3" />
    <rect x="42" y="26" width="4" height="12" rx="2" style:--bar-i="4" />
    <rect x="50" y="30" width="4" height="4" rx="2" style:--bar-i="5" />
  </g>
</svg>

<style>
  /* Animated waveform: each bar scales on its Y axis around the SVG centerline
     (y=32), staggered by --bar-i. Mirrors the visual of an audio level meter
     while STT/LLM weights load. Respects prefers-reduced-motion. */
  .lirevo-logo-loading rect {
    transform-box: fill-box;
    transform-origin: center;
    animation: lirevo-bar 1.1s ease-in-out infinite;
    animation-delay: calc(var(--bar-i) * 80ms);
  }

  @keyframes lirevo-bar {
    0%,
    100% {
      transform: scaleY(0.6);
      opacity: 0.45;
    }
    50% {
      transform: scaleY(1.15);
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .lirevo-logo-loading rect {
      animation: lirevo-bar-pulse 2s ease-in-out infinite;
      animation-delay: 0ms;
    }
    @keyframes lirevo-bar-pulse {
      0%,
      100% {
        opacity: 0.5;
      }
      50% {
        opacity: 1;
      }
    }
  }
</style>
