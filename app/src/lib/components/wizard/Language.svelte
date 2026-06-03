<script lang="ts">
  import { onMount } from 'svelte';
  import * as Select from '$lib/components/ui/select';
  import { Languages } from '@lucide/svelte';
  import { WIZARD_LANGUAGES, languageLabel, modelForLanguage } from '$lib/models/catalog';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { wizardDownloadSelection } from '$lib/stores/wizardDownloads';
  import { lda, type CatalogEntry } from '$lib/tauri';
  import { t } from '$lib/i18n';
  import { withErrorToast } from '$lib/stores/toasts';
  import { defaultStepState, type WizardStepState } from './step-state';

  interface Props {
    onnext: () => void;
    nextState?: WizardStepState;
  }
  let {
    onnext,
    nextState = $bindable(defaultStepState()),
  }: Props = $props();

  // Backend LLM catalog (same source the Settings → Models tab + the old
  // Cleanup step used) so the recommended cleanup model stays in lockstep
  // with the catalog rather than being hardcoded here.
  let catalog = $state<CatalogEntry[]>([]);
  let recommendedLlmId = $derived.by(() => {
    const llms = catalog
      .filter((c) => c.kind === 'llm')
      .toSorted((a, b) => {
        if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
        const sa = a.scores?.compositeWeighted ?? -1;
        const sb = b.scores?.compositeWeighted ?? -1;
        if (sa !== sb) return sb - sa;
        return b.sizeBytes - a.sizeBytes;
      });
    return llms.find((c) => c.recommended)?.id ?? llms[0]?.id ?? null;
  });

  // Pre-selection priority: an already-chosen dictation language, then the
  // UI language, then English — but only if the candidate is actually in
  // the curated picker list (so we never render a value with no option).
  function initialLanguage(): string {
    const candidates = [
      $settings?.language,
      $settings?.uiLanguage,
      'en',
    ].filter((c): c is string => !!c && c !== 'auto');
    const supported = new Set(WIZARD_LANGUAGES.map((l) => l.code));
    return candidates.find((c) => supported.has(c)) ?? WIZARD_LANGUAGES[0]?.code ?? 'en';
  }

  let selected = $state<string>(initialLanguage());

  onMount(async () => {
    const result = await withErrorToast(t('settings.models.error.refresh'), () =>
      lda.modelsCatalog(),
    );
    if (result !== null) catalog = result;
  });

  function onSelectChange(v: string | undefined) {
    if (v) selected = v;
  }

  async function continueNext() {
    const sttId = modelForLanguage(selected);
    const llmId = recommendedLlmId;

    await updateSettings({ language: selected, sttModelId: sttId });
    wizardDownloadSelection.set({ sttId, llmId });

    // Fire-and-forget both downloads; progress arrives via download:progress
    // events that the Downloads step watches. Errors there surface inline.
    void lda.sttDownload(sttId);
    if (llmId) void lda.modelsDownload(llmId);

    onnext();
  }

  let triggerLabel = $derived(languageLabel(selected));

  $effect(() => {
    nextState = {
      canNext: !!selected,
      onNextClick: continueNext,
    };
  });
</script>

<div class="max-w-md mx-auto flex flex-col items-center text-center gap-6">
  <div class="space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <div class="mx-auto mb-2 flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10 text-primary">
      <Languages class="h-6 w-6" />
    </div>
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.language.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.language.body')}</p>
  </div>

  <div class="w-full rounded-xl border border-border bg-surface p-4 text-left space-y-3 animate-in fade-in duration-500 delay-200">
    <div class="text-xs uppercase tracking-wide text-muted-foreground">
      {t('wizard.language.picker_label')}
    </div>

    <Select.Root type="single" value={selected} onValueChange={onSelectChange}>
      <Select.Trigger class="w-full">
        <span class="flex-1 min-w-0 truncate text-left">{triggerLabel}</span>
      </Select.Trigger>
      <Select.Content>
        {#each WIZARD_LANGUAGES as l (l.code)}
          <Select.Item value={l.code}>{l.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>
</div>
