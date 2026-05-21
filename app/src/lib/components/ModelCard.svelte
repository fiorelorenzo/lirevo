<script lang="ts">
  import { Download, X, Check, Sparkles, Trash2 } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import { Progress } from '$lib/components/ui/progress';
  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { progressFor } from '$lib/stores/downloads';
  import { lda, type CatalogEntry } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props {
    entry: CatalogEntry;
    installed: boolean;
    selected: boolean;
    onselect?: () => void;
    ondelete?: () => void | Promise<void>;
  }
  let { entry, installed, selected, onselect, ondelete }: Props = $props();

  // $derived so the store rebinds if `entry` is swapped (e.g. parent reuses
  // the component for a different catalog row). The earlier `const` form
  // captured the initial entry.id only and tripped Svelte's
  // `state_referenced_locally` warning.
  let progress = $derived(progressFor(entry.id));
  let confirmOpen = $state(false);
  let deleting = $state(false);

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

  async function confirmDelete() {
    deleting = true;
    console.info(`[ModelCard] uninstall ${entry.id}: start`);
    try {
      await lda.modelsDelete(entry.id);
      console.info(`[ModelCard] uninstall ${entry.id}: backend ok, refreshing list`);
      // Await the refresh so the UI is in sync BEFORE the dialog closes —
      // otherwise the user can see the "Installed" badge still visible for a
      // tick while the list re-fetches, which reads as "delete didn't work".
      await ondelete?.();
      console.info(`[ModelCard] uninstall ${entry.id}: refresh complete`);
    } catch (e) {
      console.error('[ModelCard] uninstall failed', e);
      // Backend also emits a toast for fatal errors; this catch keeps the
      // dialog responsive (so it can be dismissed) when the JS side throws.
    } finally {
      deleting = false;
      confirmOpen = false;
    }
  }
</script>

<div class="relative">
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

  <!--
    Delete affordance lives OUTSIDE the card-button to avoid nested
    interactive content (HTML forbids <button> inside <button>). Sits
    absolutely positioned in the bottom-right; only rendered when the
    model is installed.
  -->
  {#if installed}
    <button
      type="button"
      aria-label={t('settings.models.delete_aria', { name: entry.displayName })}
      title={t('settings.models.delete_tooltip')}
      onclick={() => (confirmOpen = true)}
      class="absolute bottom-3 right-3 p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
    >
      <Trash2 class="h-3.5 w-3.5" />
    </button>
  {/if}
</div>

<AlertDialog.Root bind:open={confirmOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>
        {t('settings.models.delete_confirm_title', { name: entry.displayName })}
      </AlertDialog.Title>
      <AlertDialog.Description>
        {t('settings.models.delete_confirm_body', { size: fmtSize(entry.sizeBytes) })}
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Action variant="destructive" disabled={deleting} onclick={confirmDelete}>
        {t('settings.models.delete_confirm_action')}
      </AlertDialog.Action>
      <AlertDialog.Cancel variant="default" disabled={deleting}>
        {t('settings.models.delete_confirm_cancel')}
      </AlertDialog.Cancel>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
