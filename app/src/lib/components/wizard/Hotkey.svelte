<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import KeyChip from '$lib/components/KeyChip.svelte';
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
</script>

<!--
  Picker layout: title + body, then a single row of medium chips, then
  the finish button. The previous design rendered the selected chip
  twice (a large preview labelled "SELECTED:" above the row) — the row
  itself already shows which one is active via the primary border +
  ring on the selected chip, so the preview was visual redundancy.
  Going down to a single row also lets the chips be bigger and easier
  to hit without crowding the dialog.
-->
<div class="min-h-full flex flex-col items-center justify-center max-w-md mx-auto gap-8 text-center">
  <div class="space-y-2">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.hotkey.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.hotkey.body')}</p>
  </div>

  <div
    class="flex flex-wrap items-center justify-center gap-3"
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

  <Button size="lg" onclick={finish}>{t('wizard.hotkey.finish')}</Button>
</div>
