<script lang="ts">
  // Reusable inline loader. Used inside buttons (during a pending action), in
  // dialog footers (while a confirmed destructive op runs), and anywhere a
  // command is in flight long enough that a passive UI would read as broken.
  //
  // Sizes match the dominant uses:
  //  - sm (12px): inline next to a label inside a small button
  //  - md (16px): default, standalone in a button
  //  - lg (20px): page-level / dialog-level affordance
  //
  // Respects `prefers-reduced-motion`: the icon stays static instead of
  // spinning. The label still updates so the user knows something is happening.
  import { Loader2 } from '@lucide/svelte';
  import { cn } from '$lib/utils';

  interface Props {
    size?: 'sm' | 'md' | 'lg';
    label?: string;
    /** Add to the wrapping span (positioning, color overrides). */
    class?: string;
    /** When true, the label is read by screen readers via aria-live. */
    announce?: boolean;
  }
  let { size = 'md', label, class: className, announce = true }: Props = $props();

  const SIZE_CLASS = { sm: 'h-3 w-3', md: 'h-4 w-4', lg: 'h-5 w-5' } as const;
</script>

<span
  class={cn('inline-flex items-center gap-1.5', className)}
  role={announce ? 'status' : undefined}
  aria-live={announce ? 'polite' : undefined}
>
  <Loader2 class={cn(SIZE_CLASS[size], 'motion-safe:animate-spin shrink-0')} aria-hidden="true" />
  {#if label}
    <span class="text-sm">{label}</span>
  {/if}
</span>
