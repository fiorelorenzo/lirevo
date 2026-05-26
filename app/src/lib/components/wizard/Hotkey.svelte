<script lang="ts">
  import KeyChip from '$lib/components/KeyChip.svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import type { Hotkey } from '$lib/tauri';
  import { t } from '$lib/i18n';
  import { defaultStepState, type WizardStepState } from './step-state';

  interface Props {
    onfinish: () => void;
    nextState?: WizardStepState;
  }
  let {
    onfinish,
    nextState = $bindable(defaultStepState()),
  }: Props = $props();

  interface Option { value: Hotkey; glyph: string; label: string; }
  const OPTIONS: Option[] = [
    { value: 'right-option',  glyph: '⌥', label: 'right' },
    { value: 'left-option',   glyph: '⌥', label: 'left' },
    { value: 'right-command', glyph: '⌘', label: 'right' },
    { value: 'fn',            glyph: 'fn', label: '' },
    { value: 'f5',            glyph: 'F5', label: '' },
  ];

  let selected = $state<Hotkey>($settings?.hotkey ?? 'right-option');

  async function finish() {
    await updateSettings({ hotkey: selected });
    onfinish();
  }

  $effect(() => {
    nextState = {
      canNext: true,
      nextLabel: t('wizard.common.done'),
      onNextClick: finish,
    };
  });
</script>

<!--
  Picker layout: title + body, then a single row of medium chips. The
  finish action lives in the wizard footer Next button — last-step
  Next reads "Done" via the nextLabel exposed above.
-->
<div class="min-h-full flex flex-col items-center justify-center max-w-md mx-auto gap-8 text-center">
  <div class="space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.hotkey.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.hotkey.body')}</p>
  </div>

  <div
    class="flex flex-wrap items-center justify-center gap-3 animate-in fade-in zoom-in duration-500 delay-200"
    role="radiogroup"
    aria-label={t('wizard.hotkey.aria_group')}
  >
    {#each OPTIONS as opt (opt.value)}
      <KeyChip
        glyph={opt.glyph}
        label={opt.label}
        size="md"
        selected={selected === opt.value}
        onclick={() => (selected = opt.value)}
      />
    {/each}
  </div>
</div>
