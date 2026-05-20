<script lang="ts">
  import { Download, X, Check, Sparkles } from '@lucide/svelte';
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

  function scoreTone(v: number): string {
    if (v >= 80) return 'text-success';
    if (v >= 50) return 'text-foreground';
    return 'text-muted-foreground';
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
      <div class="flex items-baseline gap-2 flex-wrap">
        <span class="font-medium">{entry.displayName}</span>
        <span class="text-xs text-muted-foreground tabular-nums">{fmtSize(entry.sizeBytes)}</span>
        {#if entry.recommended}
          <span
            class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[11px] font-medium leading-none"
            title="Vincitore dell'ultimo bake-off (composite score)"
          >
            <Sparkles class="h-3 w-3" />
            Recommended
          </span>
        {/if}
      </div>
      <p class="text-sm text-muted-foreground mt-1">{entry.description}</p>

      {#if entry.scores}
        {@const s = entry.scores}
        <div
          class="mt-3 grid grid-cols-4 gap-2 text-[11px] tabular-nums"
          aria-label="Benchmark scores (0-100)"
        >
          {#each [
            { label: 'Quality',  v: s.quality, hint: `chrF̄ ${s.rawChrfMean.toFixed(2)}` },
            { label: 'Latency',  v: s.latency, hint: s.rawWarmP50Ms != null ? `${s.rawWarmP50Ms} ms` : '' },
            { label: 'RAM',      v: s.ram,     hint: s.rawPeakRssKb != null ? `${Math.round(s.rawPeakRssKb / 1024)} MB` : '' },
            { label: 'Score',    v: s.compositeWeighted, hint: 'weighted composite' },
          ] as { label, v, hint } (label)}
            <div
              class="rounded-md border border-border/60 px-2 py-1.5"
              title={hint}
            >
              <div class="flex items-baseline justify-between">
                <span class="text-muted-foreground">{label}</span>
                <span class={`font-medium ${scoreTone(v)}`}>{v}</span>
              </div>
              <div class="mt-1 h-1 rounded-full bg-border/50 overflow-hidden">
                <div
                  class="h-full bg-primary transition-[width] duration-300"
                  style="width: {Math.max(0, Math.min(100, v))}%"
                ></div>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if $progress && $progress.state === 'downloading'}
        <div class="mt-3 space-y-1">
          <Progress value={($progress.bytesReceived / Math.max(1, $progress.bytesTotal)) * 100} class="h-1.5" />
          <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
            <span>{fmtSize($progress.bytesReceived)} / {fmtSize($progress.bytesTotal)}</span>
            <span>{Math.round(($progress.bytesReceived / Math.max(1, $progress.bytesTotal)) * 100)}%</span>
          </div>
        </div>
      {:else if $progress && $progress.state === 'verifying'}
        <p class="text-xs text-muted-foreground mt-3">Verifying integrity…</p>
      {:else if $progress && $progress.state === 'error'}
        <p class="text-xs text-destructive mt-3 font-mono break-words">
          {$progress.errorMessage ?? 'Download failed'}
        </p>
      {/if}
    </div>

    <div class="shrink-0">
      {#if installed}
        <div class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-success/10 text-success text-xs font-medium">
          <Check class="h-3 w-3" />
          Installed
        </div>
      {:else if $progress && ($progress.state === 'downloading' || $progress.state === 'queued')}
        <Button variant="ghost" size="sm" onclick={cancelDownload}>
          <X class="h-3 w-3 mr-1" />
          Cancel
        </Button>
      {:else if $progress && $progress.state === 'verifying'}
        <div class="text-xs text-muted-foreground px-2.5 py-1">Verifying…</div>
      {:else}
        <Button variant="outline" size="sm" onclick={startDownload}>
          <Download class="h-3 w-3 mr-1" />
          Download
        </Button>
      {/if}
    </div>
  </div>
</button>
