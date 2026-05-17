<script lang="ts">
  import { onMount } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import ModelCard from '$lib/components/ModelCard.svelte';
  import FilePicker from '$lib/components/FilePicker.svelte';
  import SkeletonRow from '$lib/components/SkeletonRow.svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { lda, type CatalogEntry, type LocalModel } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let catalog = $state<CatalogEntry[]>([]);
  let local = $state<LocalModel[]>([]);
  let loaded = $state(false);

  async function refresh() {
    [catalog, local] = await Promise.all([lda.modelsCatalog(), lda.modelsListLocal()]);
    loaded = true;
  }

  onMount(async () => {
    await refresh();
    void lda.onDownloadProgress(async (p) => {
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

  let sttReady = $derived($settings?.whisperModelPath != null);
  let llmReady = $derived($settings?.llmModelPath != null);
  let canNext = $derived(sttReady && llmReady);

  function installed(id: string): boolean {
    return local.some((l) => l.id === id);
  }
  function selectedPath(kind: 'stt' | 'llm'): string | null {
    if (!$settings) return null;
    return kind === 'stt' ? $settings.whisperModelPath : $settings.llmModelPath;
  }
  function selectModel(entry: CatalogEntry) {
    const match = local.find((l) => l.id === entry.id);
    if (!match) return;
    void updateSettings(entry.kind === 'stt'
      ? { whisperModelPath: match.path }
      : { llmModelPath: match.path });
  }

  const KINDS: ('stt' | 'llm')[] = ['stt', 'llm'];
</script>

<div class="max-w-2xl mx-auto">
  <h1 class="text-2xl font-semibold mb-2 tracking-tight">{t('wizard.models.title')}</h1>
  <p class="text-sm text-muted-foreground mb-6">{t('wizard.models.body')}</p>

  {#if loaded}
    {#each KINDS as kind (kind)}
      <section class="mb-8">
        <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
          {kind === 'stt' ? t('wizard.models.stt_section') : t('wizard.models.llm_section')}
        </h2>
        <div class="space-y-2">
          {#each catalog.filter((c) => c.kind === kind) as entry (entry.id)}
            <ModelCard
              {entry}
              installed={installed(entry.id)}
              selected={selectedPath(kind) === local.find((l) => l.id === entry.id)?.path}
              onselect={() => selectModel(entry)}
            />
          {/each}
        </div>
        <Separator class="my-4" />
        <div class="text-xs uppercase tracking-wide text-muted-foreground mb-2">
          {t('wizard.models.use_existing')}
        </div>
        <FilePicker
          value={selectedPath(kind)}
          filters={kind === 'stt'
            ? [{ name: 'Whisper ggml', extensions: ['bin'] }]
            : [{ name: 'GGUF', extensions: ['gguf'] }]}
          onpick={(p) => updateSettings(kind === 'stt' ? { whisperModelPath: p } : { llmModelPath: p })}
        />
      </section>
    {/each}

    <div class="flex justify-end mt-8">
      <Button onclick={onnext} disabled={!canNext}>{t('wizard.common.next')}</Button>
    </div>
  {:else}
    <div class="space-y-3">
      <SkeletonRow class="h-4 w-24" />
      <SkeletonRow class="h-20 w-full" />
      <SkeletonRow class="h-20 w-full" />
    </div>
  {/if}
</div>
