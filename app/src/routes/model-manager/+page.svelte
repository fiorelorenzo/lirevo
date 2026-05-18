<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { ArrowLeft } from '@lucide/svelte';
  import { Separator } from '$lib/components/ui/separator';
  import ModelCard from '$lib/components/ModelCard.svelte';
  import FilePicker from '$lib/components/FilePicker.svelte';
  import SkeletonRow from '$lib/components/SkeletonRow.svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { lda, type CatalogEntry, type LocalModel } from '$lib/tauri';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';

  let catalog = $state<CatalogEntry[]>([]);
  let local = $state<LocalModel[]>([]);
  let loaded = $state(false);
  let unlistenDownload: UnlistenFn | null = null;

  async function refresh() {
    [catalog, local] = await Promise.all([lda.modelsCatalog(), lda.modelsListLocal()]);
    loaded = true;
  }

  onMount(async () => {
    await refresh();
    unlistenDownload = await lda.onDownloadProgress(async (p) => {
      if (p.state === 'complete') {
        await refresh();
        const entry = catalog.find((c) => c.id === p.id);
        const localMatch = local.find((l) => l.id === p.id);
        if (entry && localMatch) {
          const patch = entry.kind === 'stt'
            ? { whisperModelPath: localMatch.path }
            : { llmModelPath: localMatch.path };
          await updateSettings(patch);
        }
      }
    });
  });

  onDestroy(() => {
    unlistenDownload?.();
  });

  function installed(id: string): boolean {
    return local.some((l) => l.id === id);
  }

  function selectedFor(kind: 'stt' | 'llm'): string | null {
    if (!$settings) return null;
    return kind === 'stt' ? $settings.whisperModelPath : $settings.llmModelPath;
  }

  function selectModel(entry: CatalogEntry) {
    const match = local.find((l) => l.id === entry.id);
    if (!match) return;
    const patch = entry.kind === 'stt'
      ? { whisperModelPath: match.path }
      : { llmModelPath: match.path };
    void updateSettings(patch);
  }

  function fmtSize(bytes: number): string {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }

  let usedBytes = $derived(local.reduce((s, l) => s + l.sizeBytes, 0));
  let installedCount = $derived(local.filter((l) => l.inCatalog).length);

  const KINDS: ('stt' | 'llm')[] = ['stt', 'llm'];
</script>

<div class="h-full p-8 overflow-y-auto">
  <button
    onclick={() => navigate('settings')}
    class="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors mb-4"
  >
    <ArrowLeft class="h-4 w-4" />
    {t('model_manager.back')}
  </button>
  <h1 class="text-2xl font-semibold mb-4">{t('model_manager.title')}</h1>

  {#if loaded}
    <div class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-muted/50 text-xs text-muted-foreground mb-8">
      {t('model_manager.stats', { used: fmtSize(usedBytes), installed: installedCount, total: catalog.length })}
    </div>

    {#each KINDS as kind (kind)}
      <section class="mb-10">
        <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
          {kind === 'stt' ? t('model_manager.stt_section') : t('model_manager.llm_section')}
        </h2>

        <div class="space-y-2">
          {#each catalog.filter((c) => c.kind === kind) as entry (entry.id)}
            <ModelCard
              {entry}
              installed={installed(entry.id)}
              selected={selectedFor(kind) === local.find((l) => l.id === entry.id)?.path}
              onselect={() => selectModel(entry)}
            />
          {/each}
        </div>

        <Separator class="my-4" />

        <div class="text-xs uppercase tracking-wide text-muted-foreground mb-2">
          {t('model_manager.use_existing')}
        </div>
        <FilePicker
          value={selectedFor(kind)}
          filters={kind === 'stt'
            ? [{ name: 'Whisper ggml', extensions: ['bin'] }]
            : [{ name: 'GGUF', extensions: ['gguf'] }]}
          onpick={(p) => updateSettings(kind === 'stt' ? { whisperModelPath: p } : { llmModelPath: p })}
        />
      </section>
    {/each}
  {:else}
    <div class="space-y-3">
      <SkeletonRow class="h-4 w-32" />
      <SkeletonRow class="h-16 w-full" />
      <SkeletonRow class="h-16 w-full" />
      <SkeletonRow class="h-16 w-full" />
    </div>
  {/if}
</div>
