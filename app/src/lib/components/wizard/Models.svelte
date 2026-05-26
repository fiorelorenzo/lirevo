<script lang="ts">
  import { onMount } from 'svelte';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Sparkles } from '@lucide/svelte';
  import {
    STT_MODELS,
    defaultModelId,
    formatSize,
    assertCatalogParity,
    type SttModelEntry,
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

  // Pre-select the persisted id if it's still in the catalog, otherwise
  // fall back to the catalog default. This handles two cases at once:
  //   1. First-run, sttModelId still null → defaultModelId().
  //   2. Settings file from a future build with an id we no longer ship
  //      (or a removed feature) → silently fall back to default rather
  //      than render an empty selection.
  function initialSelection(): string {
    const stored = $settings?.sttModelId ?? null;
    if (stored && STT_MODELS.some((m) => m.id === stored)) return stored;
    return defaultModelId();
  }

  let selected = $state<string>(initialSelection());

  // Dev-only: verify the static TS catalog matches what the backend will
  // load. Production builds are a no-op (see assertCatalogParity).
  // Failure throws, surfacing in the dev console + as an uncaught error
  // — exactly the loudness we want during development.
  onMount(() => {
    void assertCatalogParity();
  });

  async function continueNext() {
    // Persist the choice before navigating so the Language step (and any
    // future steps that read sttModelId) sees the new value immediately.
    // updateSettings shows its own error toast on failure; we proceed
    // even if the persist fails so the user isn't stuck on the wizard.
    await updateSettings({ sttModelId: selected });
    onnext();
  }

  function cardClasses(entry: SttModelEntry): string {
    const active = selected === entry.id;
    return [
      'relative w-full p-4 bg-surface border-2 rounded-lg text-left cursor-pointer',
      'transition-colors duration-150 hover:bg-accent/30',
      active ? 'border-primary ring-2 ring-primary/30' : 'border-border',
    ].join(' ');
  }

  $effect(() => {
    nextState = {
      canNext: !!selected,
      onNextClick: continueNext,
    };
  });
</script>

<div class="max-w-2xl mx-auto">
  <h1 class="text-2xl font-semibold mb-2 tracking-tight animate-in fade-in slide-in-from-bottom-2 duration-500">
    {t('wizard.models.title')}
  </h1>
  <p class="text-sm text-muted-foreground mb-6 animate-in fade-in duration-500 delay-100">
    {t('wizard.models.body')}
  </p>

  <div class="animate-in fade-in duration-500 delay-200">
    <RadioGroup.Root bind:value={selected} class="space-y-2">
      {#each STT_MODELS as entry (entry.id)}
        <!-- The label is the click surface; the radio control sits inside
             so keyboard focus + spacebar selection work via the native
             input. Avoids the click-handler-on-div anti-pattern. -->
        <label class={cardClasses(entry)}>
          <div class="flex items-start gap-4">
            <RadioGroup.Item value={entry.id} class="mt-1 shrink-0" />

            <div class="flex-1 min-w-0">
              <div class="flex items-baseline gap-2 flex-wrap">
                <span class="font-medium">{entry.displayName}</span>
                <span class="text-xs text-muted-foreground tabular-nums">
                  {formatSize(entry.sizeBytes)}
                </span>
                {#if entry.default}
                  <span
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[11px] font-medium leading-none"
                  >
                    <Sparkles class="h-3 w-3" />
                    {t('wizard.models.recommended_pill')}
                  </span>
                {/if}
              </div>
              <p class="text-sm text-muted-foreground mt-1">{entry.summary}</p>
              <div class="mt-2 inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <span class="px-1.5 py-0.5 rounded border border-border/60 font-mono leading-none">
                  {entry.license}
                </span>
              </div>
            </div>
          </div>
        </label>
      {/each}
    </RadioGroup.Root>
  </div>
</div>
