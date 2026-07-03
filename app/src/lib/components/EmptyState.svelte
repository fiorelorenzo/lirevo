<script lang="ts">
  import type { Component } from "svelte";

  interface Props {
    icon?: Component;
    title: string;
    body?: string;
    cta?: { label: string; onclick: () => void };
  }
  let { icon: Icon, title, body, cta }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center py-16 px-8 text-center">
  {#if Icon}
    <div class="w-16 h-16 rounded-full bg-primary/10 flex items-center justify-center mb-4">
      <Icon class="w-8 h-8 text-primary" />
    </div>
  {/if}
  <h3 class="text-lg font-semibold mb-2">{title}</h3>
  {#if body}<p class="text-sm text-muted-foreground max-w-sm mb-6">{body}</p>{/if}
  {#if cta}
    <button
      onclick={cta.onclick}
      class="px-4 py-2 rounded-md bg-primary text-primary-foreground font-medium text-sm hover:bg-[var(--color-primary-hover)] transition-colors"
    >
      {cta.label}
    </button>
  {/if}
</div>
