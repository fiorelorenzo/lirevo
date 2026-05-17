<script lang="ts">
  interface Props { size?: number; class?: string; }
  let { size = 64, class: cls = '' }: Props = $props();
</script>

<svg viewBox="0 0 64 64" width={size} height={size} class={cls} aria-hidden="true">
  <defs>
    <linearGradient id="lda-logo-grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="var(--color-primary)" />
      <stop offset="100%" stop-color="var(--color-accent-violet)" />
    </linearGradient>
  </defs>
  <g fill="url(#lda-logo-grad)">
    <rect x="10" y="28" width="4" height="8" rx="2" />
    <rect x="18" y="22" width="4" height="20" rx="2" />
    <rect x="26" y="16" width="4" height="32" rx="2" />
    <rect x="34" y="20" width="4" height="24" rx="2" />
    <rect x="42" y="26" width="4" height="12" rx="2" />
    <rect x="50" y="30" width="4" height="4" rx="2" />
  </g>
</svg>
