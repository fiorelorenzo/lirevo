<script lang="ts">
  import { onMount } from 'svelte';
  import * as Select from '$lib/components/ui/select';
  import { Info } from '@lucide/svelte';
  import {
    defaultModelId,
    findModel,
    languagesForModel,
  } from '$lib/models/catalog';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { t } from '$lib/i18n';
  import { defaultStepState, type WizardStepState } from './step-state';

  interface Props {
    onnext: () => void;
    nextState?: WizardStepState;
  }
  let {
    onnext,
    nextState = $bindable(defaultStepState()),
  }: Props = $props();

  // Reuses the flat `settings.language` field (camelCase: `language`) that
  // the existing dictation pipeline already consumes via
  // `normalize_language` in inference.rs — `""` and `"auto"` both mean
  // auto-detect there. We standardize on `"auto"` here so the dropdown
  // value is meaningful in logs.
  const AUTO = 'auto';

  // ISO-639-1 + a few 639-2 (yue, fil) display names. Kept inline rather
  // than pulled from a library because the universe is small (the union
  // of Parakeet + Qwen3 + curated Whisper) and a runtime locale-display
  // dependency would dwarf the strings themselves.
  const LANGUAGE_NAMES: Record<string, string> = {
    ar: 'Arabic',
    bg: 'Bulgarian',
    cs: 'Czech',
    da: 'Danish',
    de: 'German',
    el: 'Greek',
    en: 'English',
    es: 'Spanish',
    et: 'Estonian',
    fa: 'Persian',
    fi: 'Finnish',
    fil: 'Filipino',
    fr: 'French',
    he: 'Hebrew',
    hi: 'Hindi',
    hr: 'Croatian',
    hu: 'Hungarian',
    id: 'Indonesian',
    it: 'Italian',
    ja: 'Japanese',
    ko: 'Korean',
    lt: 'Lithuanian',
    lv: 'Latvian',
    mk: 'Macedonian',
    ms: 'Malay',
    mt: 'Maltese',
    nl: 'Dutch',
    no: 'Norwegian',
    pl: 'Polish',
    pt: 'Portuguese',
    ro: 'Romanian',
    ru: 'Russian',
    sk: 'Slovak',
    sl: 'Slovenian',
    sv: 'Swedish',
    sw: 'Swahili',
    th: 'Thai',
    tr: 'Turkish',
    uk: 'Ukrainian',
    vi: 'Vietnamese',
    yue: 'Cantonese',
    zh: 'Chinese',
  };

  function labelFor(code: string): string {
    return LANGUAGE_NAMES[code] ?? code.toUpperCase();
  }

  // Resolve the active model id once per mount. Defaults to the catalog
  // default for the (legitimate) case where the user skipped the Models
  // step or arrived here from a settings-shortcut path.
  let modelId = $derived($settings?.sttModelId ?? defaultModelId());
  let model = $derived(findModel(modelId));

  // Sorted alphabetically by display name. The Auto-detect option is
  // prepended as a separate Select item below — keeping it out of this
  // list avoids it sorting into the middle.
  let supportedLanguages = $derived(
    languagesForModel(modelId)
      .map((code) => ({ code, label: labelFor(code) }))
      .sort((a, b) => a.label.localeCompare(b.label)),
  );

  let supportedCodes = $derived(new Set(supportedLanguages.map((l) => l.code)));

  let selected = $state<string>(AUTO);
  let resetNotice = $state(false);

  onMount(() => {
    // Read the persisted language. If it was set to an ISO code that the
    // newly-chosen model can't decode (user went back, swapped to a
    // narrower model, came forward again), fall back to auto and surface
    // an inline notice so they understand why the dropdown reverted.
    const persisted = $settings?.language ?? AUTO;
    if (persisted === AUTO || persisted === '') {
      selected = AUTO;
      return;
    }
    if (supportedCodes.has(persisted)) {
      selected = persisted;
    } else {
      selected = AUTO;
      resetNotice = true;
      // Persist the rollback so the next reload doesn't see the stale
      // code in settings.json.
      void updateSettings({ language: AUTO });
    }
  });

  function onSelectChange(v: string | undefined) {
    if (!v) return;
    selected = v;
    resetNotice = false;
    void updateSettings({ language: v });
  }

  function continueNext() {
    onnext();
  }

  let triggerLabel = $derived(
    selected === AUTO ? t('wizard.language.auto_label') : labelFor(selected),
  );

  $effect(() => {
    nextState = {
      canNext: true,
      onNextClick: continueNext,
    };
  });
</script>

<div class="max-w-md mx-auto flex flex-col items-center text-center gap-6">
  <div class="space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.language.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.language.body')}</p>
  </div>

  {#if model}
    <div class="w-full rounded-xl border border-border bg-surface p-4 text-left space-y-3 animate-in fade-in duration-500 delay-200">
      <div class="text-xs uppercase tracking-wide text-muted-foreground">
        {t('wizard.language.model_label')}
      </div>
      <div class="font-medium">{model.displayName}</div>

      <Select.Root
        type="single"
        value={selected}
        onValueChange={onSelectChange}
      >
        <Select.Trigger class="w-full">
          <span class="flex-1 min-w-0 truncate text-left">{triggerLabel}</span>
        </Select.Trigger>
        <Select.Content>
          <Select.Item value={AUTO}>{t('wizard.language.auto_label')}</Select.Item>
          {#each supportedLanguages as l (l.code)}
            <Select.Item value={l.code}>{l.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>

      {#if resetNotice}
        <div
          class="flex items-start gap-2 text-xs text-muted-foreground rounded-md border border-border/60 bg-muted/30 p-2"
          role="status"
        >
          <Info class="h-3.5 w-3.5 shrink-0 mt-0.5" />
          <span>{t('wizard.language.reset_notice')}</span>
        </div>
      {/if}
    </div>
  {/if}
</div>
