<script lang="ts">
  import { Download, X, Check } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import { Progress } from '$lib/components/ui/progress';
  import { progressFor } from '$lib/stores/downloads';
  import { lda, type CatalogEntry } from '$lib/tauri';

  interface Props {
    entry: CatalogEntry;
    installed: boolean;
    selected: boolean;
    onselect?: () => void;
  }
  let { entry, installed, selected, onselect }: Props = $props();

  // $derived so the store rebinds if `entry` is swapped (e.g. parent reuses
  // the component for a different catalog row). The earlier `const` form
  // captured the initial entry.id only and tripped Svelte's
  // `state_referenced_locally` warning.
  let progress = $derived(progressFor(entry.id));

  function fmtSize(bytes: number): string {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }

  async function startDownload() {
    try {
      await lda.modelsDownload(entry.id);
    } catch (e) {
      console.error(e);
    }
  }

  async function cancelDownload() {
    try {
      await lda.modelsCancelDownload(entry.id);
    } catch (e) {
      console.error(e);
    }
  }
</script>

<button
  type="button"
  onclick={() => installed && onselect?.()}
  class={[
    'w-full p-4 bg-surface border-2 rounded-lg text-left transition-all duration-150',
    'hover:-translate-y-px hover:shadow-md',
    selected ? 'border-primary ring-2 ring-primary/30' : 'border-border hover:border-border-strong',
    installed ? 'cursor-pointer' : 'cursor-default',
  ].join(' ')}
>
  <div class="flex items-start gap-4">
    <div class="flex-1 min-w-0">
      <div class="flex items-baseline gap-2">
        <span class="font-medium">{entry.displayName}</span>
        <span class="text-xs text-muted-foreground tabular-nums">{fmtSize(entry.sizeBytes)}</span>
      </div>
      <p class="text-sm text-muted-foreground mt-1">{entry.description}</p>

      {#if $progress && $progress.state === 'downloading'}
        <div class="mt-3 space-y-1">
          <Progress value={($progress.bytesReceived / Math.max(1, $progress.bytesTotal)) * 100} class="h-1.5" />
          <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
            <span>{fmtSize($progress.bytesReceived)} / {fmtSize($progress.bytesTotal)}</span>
            <span>{Math.round(($progress.bytesReceived / Math.max(1, $progress.bytesTotal)) * 100)}%</span>
          </div>
        </div>
      {/if}
    </div>

    <div class="shrink-0">
      {#if installed}
        <div class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-success/10 text-success text-xs font-medium">
          <Check class="h-3 w-3" />
          Installed
        </div>
      {:else if $progress && $progress.state === 'downloading'}
        <Button variant="ghost" size="sm" onclick={cancelDownload}>
          <X class="h-3 w-3 mr-1" />
          Cancel
        </Button>
      {:else}
        <Button variant="outline" size="sm" onclick={startDownload}>
          <Download class="h-3 w-3 mr-1" />
          Download
        </Button>
      {/if}
    </div>
  </div>
</button>
