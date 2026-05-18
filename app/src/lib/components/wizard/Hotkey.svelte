<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import KeyChip from '$lib/components/KeyChip.svelte';
  import { Sparkles } from '@lucide/svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import type { Hotkey } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onfinish: () => void; }
  let { onfinish }: Props = $props();

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

  let selectedOption = $derived(OPTIONS.find((o) => o.value === selected) ?? OPTIONS[0]);
</script>

<div class="min-h-full flex flex-col items-center justify-center max-w-md mx-auto gap-8 text-center">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.hotkey.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.hotkey.body')}</p>

  <div class="flex flex-col items-center gap-3">
    <span class="text-xs uppercase tracking-wide text-muted-foreground">{t('wizard.hotkey.selected')}</span>
    <KeyChip
      glyph={selectedOption.glyph}
      label={selectedOption.label}
      size="lg"
      selected
    />
  </div>

  <div class="flex flex-wrap items-center justify-center gap-3">
    {#each OPTIONS as opt (opt.value)}
      <KeyChip
        glyph={opt.glyph}
        label={opt.label}
        size="sm"
        selected={selected === opt.value}
        onclick={() => (selected = opt.value)}
      />
    {/each}
  </div>

  <Button size="lg" onclick={finish}>
    <Sparkles class="h-4 w-4 mr-2" />
    {t('wizard.hotkey.finish')}
  </Button>
</div>
