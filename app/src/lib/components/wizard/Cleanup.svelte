<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Progress } from '$lib/components/ui/progress';
  import { Sparkles, Check, Download as DownloadIcon } from '@lucide/svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { lda, type CatalogEntry, type LocalModel } from '$lib/tauri';
  import { progressFor } from '$lib/stores/downloads';
  import { t } from '$lib/i18n';
  import { withErrorToast } from '$lib/stores/toasts';
  import type { UnlistenFn } from '@tauri-apps/api/event';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  // Pseudo-id for the "Skip" radio option. Kept distinct from any catalog id
  // so the selection state is unambiguous even if a future catalog adds an
  // entry with a generic id.
  const SKIP_ID = '__skip__';

  // Catalog ids that this step offers. Must exist in the M3 LLM catalog
  // (crates/inference-core/data/model_catalog.json). Order matters: first
  // entry is recommended.
  const LLAMA_ID = 'llama-3.2-3b-instruct-q4';
  const QWEN_ID = 'qwen3-4b-instruct-2507-q4';

  let catalog = $state<CatalogEntry[]>([]);
  let local = $state<LocalModel[]>([]);
  let loaded = $state(false);
  let unlistenDownload: UnlistenFn | null = null;

  // Track the id the user picked but hasn't yet downloaded — used to drive
  // a "downloading" state on Continue.
  let downloadingId = $state<string | null>(null);

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
  // the recommended Llama option.
  function initialSelection(): string {
    const stored = $settings?.llmModelPath ?? null;
    if (!stored) return LLAMA_ID;
    const match = local.find((l) => l.path === stored);
    if (match && (match.id === LLAMA_ID || match.id === QWEN_ID)) {
      return match.id;
    }
    return LLAMA_ID;
  }

  let selected = $state<string>(LLAMA_ID);

  onMount(async () => {
    const result = await withErrorToast(t('settings.models.error.refresh'), () =>
      Promise.all([lda.modelsCatalog(), lda.modelsListLocal()]),
    );
    if (result !== null) {
      [catalog, local] = result;
    }
    loaded = true;
    // Re-evaluate the initial selection now that local + settings are
    // resolved.
    selected = initialSelection();

    unlistenDownload = await lda.onDownloadProgress(async (p) => {
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

  let llamaProgress = $derived(progressFor(LLAMA_ID));
  let qwenProgress = $derived(progressFor(QWEN_ID));

  function progressStoreFor(id: string) {
    if (id === LLAMA_ID) return llamaProgress;
    if (id === QWEN_ID) return qwenProgress;
    return null;
  }

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

  let llama = $derived(entryById(LLAMA_ID));
  let qwen = $derived(entryById(QWEN_ID));

  let activeProgress = $derived(
    downloadingId ? progressStoreFor(downloadingId) : null,
  );
  let isDownloading = $derived(downloadingId !== null);
  let continueDisabled = $derived(!loaded || isDownloading);

  function statusPill(id: string): { label: string; tone: 'muted' | 'success' | 'progress' } | null {
    if (id === SKIP_ID) {
      return { label: t('wizard.cleanup.skip_pill'), tone: 'muted' };
    }
    if (isInstalled(id)) {
      return { label: t('wizard.cleanup.downloaded_pill'), tone: 'success' };
    }
    const p = id === LLAMA_ID ? $llamaProgress : id === QWEN_ID ? $qwenProgress : undefined;
    if (p && (p.state === 'downloading' || p.state === 'queued' || p.state === 'verifying')) {
      return { label: t('wizard.cleanup.downloading_pill'), tone: 'progress' };
    }
    return null;
  }
</script>

<div class="max-w-2xl mx-auto">
  <h1 class="text-2xl font-semibold mb-2 tracking-tight">{t('wizard.cleanup.title')}</h1>
  <p class="text-sm text-muted-foreground mb-6">{t('wizard.cleanup.body')}</p>

  <RadioGroup.Root bind:value={selected} class="space-y-2">
    {#if llama}
      {@const pill = statusPill(LLAMA_ID)}
      <label class={cardClasses(selected === LLAMA_ID)}>
        <div class="flex items-start gap-4">
          <RadioGroup.Item value={LLAMA_ID} class="mt-1 shrink-0" />
          <div class="flex-1 min-w-0">
            <div class="flex items-baseline gap-2 flex-wrap">
              <span class="font-medium">{llama.displayName}</span>
              <span class="text-xs text-muted-foreground tabular-nums">
                {fmtSize(llama.sizeBytes)}
              </span>
              <span
                class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[11px] font-medium leading-none"
              >
                <Sparkles class="h-3 w-3" />
                {t('wizard.cleanup.recommended_pill')}
              </span>
              {#if pill}
                <span
                  class={[
                    'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium leading-none',
                    pill.tone === 'success'
                      ? 'bg-success/10 text-success'
                      : pill.tone === 'progress'
                        ? 'bg-muted text-muted-foreground'
                        : 'bg-muted text-muted-foreground',
                  ].join(' ')}
                >
                  {#if pill.tone === 'success'}<Check class="h-3 w-3" />{/if}
                  {pill.label}
                </span>
              {/if}
            </div>
            <p class="text-sm text-muted-foreground mt-1">{llama.description}</p>
          </div>
        </div>
      </label>
    {/if}

    {#if qwen}
      {@const pill = statusPill(QWEN_ID)}
      <label class={cardClasses(selected === QWEN_ID)}>
        <div class="flex items-start gap-4">
          <RadioGroup.Item value={QWEN_ID} class="mt-1 shrink-0" />
          <div class="flex-1 min-w-0">
            <div class="flex items-baseline gap-2 flex-wrap">
              <span class="font-medium">{qwen.displayName}</span>
              <span class="text-xs text-muted-foreground tabular-nums">
                {fmtSize(qwen.sizeBytes)}
              </span>
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
            <p class="text-sm text-muted-foreground mt-1">{qwen.description}</p>
          </div>
        </div>
      </label>
    {/if}

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
