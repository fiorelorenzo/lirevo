<script lang="ts">
  import { slide } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import { Trash2, AppWindow, ChevronDown } from '@lucide/svelte';
  import type { DictationSummary } from '$lib/tauri';
  import { formatRelative, formatMs } from './format';
  import HistoryDetail from './HistoryDetail.svelte';

  interface Props {
    items: DictationSummary[];
    selectedId: number | null;
    onSelect: (id: number) => void;
    onDelete: (id: number) => void;
  }
  let { items, selectedId, onSelect, onDelete }: Props = $props();

  // Tick so relative times age live ("just now" -> "1m" -> "2h") instead of
  // freezing at whatever they were when the row first rendered.
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(t);
  });
</script>

<ul class="flex flex-col gap-1.5">
  {#each items as item (item.id)}
    {@const isSelected = item.id === selectedId}
    <li>
      <div
        class={[
          'overflow-hidden rounded-lg border transition-colors',
          isSelected ? 'border-primary/40 bg-surface' : 'border-border/60 bg-surface',
        ].join(' ')}
      >
        <div
          class={[
            'group relative flex items-center gap-3 px-3 py-2.5 text-left transition-colors',
            isSelected ? '' : 'hover:bg-accent/30',
          ].join(' ')}
        >
        <button
          type="button"
          onclick={() => onSelect(item.id)}
          class="flex min-w-0 flex-1 flex-col gap-1.5 text-left"
          aria-expanded={isSelected}
        >
          <span class="truncate text-sm text-foreground">
            {item.preview || '(empty)'}
          </span>
          <span class="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <span class="tabular-nums">{formatRelative(item.createdAt, now)}</span>
            <span class="text-border" aria-hidden="true">·</span>
            <span
              class="rounded-full border border-border/60 bg-muted/40 px-1.5 py-0.5 font-medium"
            >
              {item.sttModel}
            </span>
            <span
              class="rounded-full border border-border/60 bg-muted/40 px-1.5 py-0.5 font-medium"
            >
              {item.llmModel ?? 'raw'}
            </span>
            {#if item.targetApp}
              <span
                class="hidden items-center gap-1 rounded-full border border-border/60 bg-muted/40 px-1.5 py-0.5 font-medium sm:inline-flex"
              >
                <AppWindow class="h-3 w-3" />
                {item.targetApp}
              </span>
            {/if}
            <span class="tabular-nums">{formatMs(item.totalMs)}</span>
          </span>
        </button>

        <button
          type="button"
          onclick={() => onDelete(item.id)}
          aria-label="Delete dictation"
          class="shrink-0 rounded-md p-1.5 text-muted-foreground opacity-0 transition-all hover:bg-destructive/10 hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
        >
          <Trash2 class="h-3.5 w-3.5" />
        </button>
        <ChevronDown
          class={[
            'h-4 w-4 shrink-0 text-muted-foreground transition-transform',
            isSelected ? 'rotate-180' : '',
          ].join(' ')}
        />
        </div>

        {#if isSelected}
          <div
            transition:slide={{ duration: 220, easing: quintOut }}
            class="border-t border-border/60 px-3 py-3"
          >
            <HistoryDetail id={item.id} />
          </div>
        {/if}
      </div>
    </li>
  {/each}
</ul>
