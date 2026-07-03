<script lang="ts">
  import { Switch as SwitchPrimitive } from "bits-ui";
  import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    class: className,
    checked = $bindable(false),
    size = "default",
    ...restProps
  }: WithoutChildrenOrChild<SwitchPrimitive.RootProps> & {
    size?: "sm" | "default";
  } = $props();
</script>

<!--
bits-ui emits `data-state="checked|unchecked"`, NOT `data-checked` /
`data-unchecked`. The shadcn template's `data-checked:` shorthand matches
an attribute that doesn't exist, so the track stays transparent in both
states. Use the explicit `data-[state=...]:` form throughout.
-->
<SwitchPrimitive.Root
  bind:ref
  bind:checked
  data-slot="switch"
  data-size={size}
  class={cn(
    "bg-input data-[state=checked]:bg-primary focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 aria-invalid:border-destructive shrink-0 rounded-full border border-transparent focus-visible:ring-3 aria-invalid:ring-3 data-[size=default]:h-5 data-[size=default]:w-9 data-[size=sm]:h-4 data-[size=sm]:w-7 peer group/switch relative inline-flex items-center transition-colors outline-none data-disabled:cursor-not-allowed data-disabled:opacity-50",
    className,
  )}
  {...restProps}
>
  <SwitchPrimitive.Thumb
    data-slot="switch-thumb"
    class="bg-background rounded-full shadow-sm group-data-[size=default]/switch:size-4 group-data-[size=sm]/switch:size-3 group-data-[state=checked]/switch:translate-x-[calc(100%-2px)] group-data-[state=unchecked]/switch:translate-x-0.5 pointer-events-none block ring-0 transition-transform"
  />
</SwitchPrimitive.Root>
