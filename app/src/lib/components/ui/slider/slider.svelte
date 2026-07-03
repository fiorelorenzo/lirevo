<script lang="ts">
  import { Slider as SliderPrimitive } from "bits-ui";
  import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    value = $bindable(),
    orientation = "horizontal",
    class: className,
    ...restProps
  }: WithoutChildrenOrChild<SliderPrimitive.RootProps> = $props();
</script>

<!--
Horizontal-only for now: the data-horizontal: / data-vertical: variants
that ship in the shadcn-svelte template don't match bits-ui's data
attribute, so the track and range both collapse to zero size. Discriminated
unions + destructuring (required for bindable) also fight typescript here,
so we cast value to `never`.
-->
<SliderPrimitive.Root
  bind:ref
  bind:value={value as never}
  data-slot="slider"
  {orientation}
  class={cn(
    "relative flex w-full touch-none items-center select-none data-disabled:opacity-50",
    className,
  )}
  {...restProps}
>
  {#snippet children({ thumbItems })}
    <span
      data-slot="slider-track"
      class="bg-input h-1.5 w-full rounded-full relative grow overflow-hidden"
    >
      <SliderPrimitive.Range
        data-slot="slider-range"
        class="bg-primary absolute h-full select-none"
      />
    </span>
    {#each thumbItems as thumb (thumb.index)}
      <SliderPrimitive.Thumb
        data-slot="slider-thumb"
        index={thumb.index}
        class="border-primary ring-ring/50 relative size-4 rounded-full border-2 bg-background shadow-sm transition-[color,box-shadow] after:absolute after:-inset-2 hover:ring-3 focus-visible:ring-3 focus-visible:outline-hidden active:ring-3 block shrink-0 select-none disabled:pointer-events-none disabled:opacity-50"
      />
    {/each}
  {/snippet}
</SliderPrimitive.Root>
