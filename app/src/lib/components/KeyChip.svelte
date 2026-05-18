<script lang="ts">
  interface Props {
    label: string;
    glyph?: string;
    size?: 'sm' | 'md' | 'lg';
    selected?: boolean;
    onclick?: () => void;
  }
  let { label, glyph, size = 'md', selected = false, onclick }: Props = $props();

  const sizes = {
    sm: 'w-14 h-14 text-xs',
    md: 'w-20 h-20 text-base',
    lg: 'w-24 h-24 text-lg',
  };
</script>

<button
  type="button"
  onclick={() => onclick?.()}
  class={[
    sizes[size],
    'flex flex-col items-center justify-center gap-1',
    'bg-surface border-2 rounded-xl font-mono',
    'transition-all duration-150 ease-out',
    'hover:scale-[1.04] hover:shadow-md',
    'active:scale-[0.96] active:duration-75',
    selected
      ? 'border-primary bg-primary/10 shadow-md ring-2 ring-primary/30'
      : 'border-border hover:border-border-strong',
  ].join(' ')}
>
  {#if glyph}<span class="text-2xl leading-none">{glyph}</span>{/if}
  <span class="font-sans text-xs text-muted-foreground">{label}</span>
</button>
