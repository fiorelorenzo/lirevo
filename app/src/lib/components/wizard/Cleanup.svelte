<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Progress } from '$lib/components/ui/progress';
  import { Sparkles, Check, Download as DownloadIcon } from '@lucide/svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { lda, type CatalogEntry, type LocalModel, type DownloadProgress } from '$lib/tauri';
  import { downloads, progressFor } from '$lib/stores/downloads';
  import { t } from '$lib/i18n';
  import { withErrorToast } from '$lib/stores/toasts';
  import type { UnlistenFn } from '@tauri-apps/api/event';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  // Pseudo-id for the "Skip" radio option. Kept distinct from any catalog id
  // so the selection state is unambiguous even if a future catalog adds an
  // entry with a generic id.
  const SKIP_ID = '__skip__';

  let catalog = $state<CatalogEntry[]>([]);
  let local = $state<LocalModel[]>([]);
  let loaded = $state(false);
  let unlistenDownload: UnlistenFn | null = null;

  // Track the id the user picked but hasn't yet downloaded — used to drive
  // a "downloading" state on Continue.
  let downloadingId = $state<string | null>(null);

  // Data-driven LLM list from the backend catalog (same source the Settings
  // → Models tab uses). Keeps wizard + settings in sync as the catalog
  // evolves; no hardcoded ids to drift.
  let llmEntries = $derived(catalog.filter((c) => c.kind === 'llm'));
  let recommendedId = $derived(
    llmEntries.find((c) => c.recommended)?.id ?? llmEntries[0]?.id ?? null,
  );

  async function refreshLocal() {
    try {
      local = await lda.modelsListLocal();
    } catch {
      // Non-fatal: caller decides whether the absence is blocking.
    }
  }

  function entryById(id: string): CatalogEntry | undefined {
    return catalog.find((c) => c.id === id);
  }

  function localById(id: string): LocalModel | undefined {
    return local.find((l) => l.id === id);
  }

  function isInstalled(id: string): boolean {
    return localById(id) !== undefined;
  }

  // Pre-selection: if the user already has an LLM configured (e.g. came
  // back via re-run wizard), pre-select that card. Otherwise default to
  // the catalog's recommended entry, falling back to the first LLM.
  function initialSelection(): string {
    const stored = $settings?.llmModelPath ?? null;
    if (stored) {
      const match = local.find((l) => l.path === stored);
      if (match && llmEntries.some((e) => e.id === match.id)) {
        return match.id;
      }
    }
    return recommendedId ?? SKIP_ID;
  }

  let selected = $state<string>(SKIP_ID);

  onMount(async () => {
    const result = await withErrorToast(t('settings.models.error.refresh'), () =>
      Promise.all([lda.modelsCatalog(), lda.modelsListLocal()]),
    );
    if (result !== null) {
      [catalog, local] = result;
    }
    loaded = true;
    // Re-evaluate the initial selection now that catalog + local + settings
    // are resolved.
    selected = initialSelection();

    unlistenDownload = await lda.onDownloadProgress(async (p: DownloadProgress) => {
      if (p.state === 'complete') {
        await refreshLocal();
        if (downloadingId === p.id) {
          const match = localById(p.id);
          if (match) {
            await updateSettings({ llmModelPath: match.path });
          }
          downloadingId = null;
          onnext();
        }
      } else if (p.state === 'error' || p.state === 'cancelled') {
        if (downloadingId === p.id) downloadingId = null;
      }
    });
  });

  onDestroy(() => {
    unlistenDownload?.();
  });

  async function continueNext() {
    if (selected === SKIP_ID) {
      await updateSettings({ llmModelPath: null });
      onnext();
      return;
    }

    // Already-installed model → just persist the path and move on.
    if (isInstalled(selected)) {
      const match = localById(selected);
      if (match) {
        await updateSettings({ llmModelPath: match.path });
      }
      onnext();
      return;
    }

    // Need to download first. The progress handler in onMount picks up the
    // `complete` event, persists the path, and advances the wizard.
    downloadingId = selected;
    const result = await withErrorToast(
      t('settings.models.download_failed', { name: entryById(selected)?.displayName ?? selected }),
      () => lda.modelsDownload(selected),
    );
    if (result === null) {
      downloadingId = null;
    }
  }

  function cardClasses(active: boolean): string {
    return [
      'relative w-full p-4 bg-surface border-2 rounded-lg text-left cursor-pointer',
      'transition-colors duration-150 hover:bg-accent/30',
      active ? 'border-primary ring-2 ring-primary/30' : 'border-border',
    ].join(' ');
  }

  function fmtSize(bytes: number): string {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }

  let activeProgress = $derived(
    downloadingId ? progressFor(downloadingId) : null,
  );
  let isDownloading = $derived(downloadingId !== null);
  let continueDisabled = $derived(!loaded || isDownloading);

  function statusPill(
    id: string,
    progress: DownloadProgress | undefined,
  ): { label: string; tone: 'muted' | 'success' | 'progress' } | null {
    if (id === SKIP_ID) {
      return { label: t('wizard.cleanup.skip_pill'), tone: 'muted' };
    }
    if (isInstalled(id)) {
      return { label: t('wizard.cleanup.downloaded_pill'), tone: 'success' };
    }
    if (progress && (progress.state === 'downloading' || progress.state === 'queued' || progress.state === 'verifying')) {
      return { label: t('wizard.cleanup.downloading_pill'), tone: 'progress' };
    }
    return null;
  }
</script>

<div class="max-w-2xl mx-auto">
  <h1 class="text-2xl font-semibold mb-2 tracking-tight">{t('wizard.cleanup.title')}</h1>
  <p class="text-sm text-muted-foreground mb-6">{t('wizard.cleanup.body')}</p>

  <RadioGroup.Root bind:value={selected} class="space-y-2">
    {#each llmEntries as entry (entry.id)}
      {@const pill = statusPill(entry.id, $downloads[entry.id])}
      {@const isRecommended = entry.id === recommendedId}
      <label class={cardClasses(selected === entry.id)}>
        <div class="flex items-start gap-4">
          <RadioGroup.Item value={entry.id} class="mt-1 shrink-0" />
          <div class="flex-1 min-w-0">
            <div class="flex items-baseline gap-2 flex-wrap">
              <span class="font-medium">{entry.displayName}</span>
              <span class="text-xs text-muted-foreground tabular-nums">
                {fmtSize(entry.sizeBytes)}
              </span>
              {#if isRecommended}
                <span
                  class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[11px] font-medium leading-none"
                >
                  <Sparkles class="h-3 w-3" />
                  {t('wizard.cleanup.recommended_pill')}
                </span>
              {/if}
              {#if pill}
                <span
                  class={[
                    'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium leading-none',
                    pill.tone === 'success'
                      ? 'bg-success/10 text-success'
                      : 'bg-muted text-muted-foreground',
                  ].join(' ')}
                >
                  {#if pill.tone === 'success'}<Check class="h-3 w-3" />{/if}
                  {pill.label}
                </span>
              {/if}
            </div>
            <p class="text-sm text-muted-foreground mt-1">{entry.description}</p>
          </div>
        </div>
      </label>
    {/each}

    <label class={cardClasses(selected === SKIP_ID)}>
      <div class="flex items-start gap-4">
        <RadioGroup.Item value={SKIP_ID} class="mt-1 shrink-0" />
        <div class="flex-1 min-w-0">
          <div class="flex items-baseline gap-2 flex-wrap">
            <span class="font-medium">{t('wizard.cleanup.skip_title')}</span>
            <span
              class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-muted text-muted-foreground text-[11px] font-medium leading-none"
            >
              {t('wizard.cleanup.skip_pill')}
            </span>
          </div>
          <p class="text-sm text-muted-foreground mt-1">{t('wizard.cleanup.skip_body')}</p>
        </div>
      </div>
    </label>
  </RadioGroup.Root>

  {#if isDownloading && $activeProgress && $activeProgress.state === 'downloading'}
    <div class="mt-6 space-y-1">
      <Progress
        value={($activeProgress.bytesReceived / Math.max(1, $activeProgress.bytesTotal)) * 100}
        class="h-1.5"
      />
      <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
        <span>{fmtSize($activeProgress.bytesReceived)} / {fmtSize($activeProgress.bytesTotal)}</span>
        <span>
          {Math.round(($activeProgress.bytesReceived / Math.max(1, $activeProgress.bytesTotal)) * 100)}%
        </span>
      </div>
    </div>
  {/if}

  <div class="flex justify-end mt-8">
    <Button onclick={continueNext} disabled={continueDisabled}>
      {#if isDownloading}
        {t('wizard.cleanup.downloading_pill')}
      {:else if selected !== SKIP_ID && !isInstalled(selected)}
        <DownloadIcon class="h-3.5 w-3.5 mr-1.5" />
        {t('wizard.common.next')}
      {:else}
        {t('wizard.common.next')}
      {/if}
    </Button>
  </div>
</div>
