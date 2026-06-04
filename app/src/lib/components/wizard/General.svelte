<script lang="ts">
  // Final wizard step: the hotkey picker plus the three "how should this
  // app live on your Mac" toggles, merged from the old Hotkey +
  // BackgroundMode steps. Finishing here completes the wizard.
  import KeyChip from '$lib/components/KeyChip.svelte';
  import { Label } from '$lib/components/ui/label';
  import { Switch } from '$lib/components/ui/switch';
  import { Rocket, EyeOff, MonitorOff } from '@lucide/svelte';
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

  // Local mirrors so toggles feel instant; updateSettings runs on change so
  // persisted state matches the UI even if the user bails via Skip.
  let launchAtLogin = $derived($settings?.launchAtLogin ?? false);
  let launchMinimized = $derived($settings?.launchMinimized ?? false);
  let stayRunningOnWindowClose = $derived(
    $settings?.stayRunningOnWindowClose ?? true,
  );

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

<div class="max-w-md mx-auto flex flex-col gap-6">
  <div class="text-center space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.general.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.general.body')}</p>
  </div>

  <div class="w-full space-y-2 animate-in fade-in duration-500 delay-100">
    <div class="text-xs uppercase tracking-wide text-muted-foreground">
      {t('wizard.general.hotkey_label')}
    </div>
    <div
      class="flex flex-wrap items-center gap-3"
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

  <div class="w-full rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden text-left animate-in fade-in duration-500 delay-200">
    <div class="p-4 flex items-start justify-between gap-4">
      <div class="flex items-start gap-3 min-w-0">
        <Rocket class="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
        <div class="min-w-0">
          <Label>{t('wizard.background.launch_at_login')}</Label>
          <p class="text-xs text-muted-foreground mt-1">
            {t('wizard.background.launch_at_login_helper')}
          </p>
        </div>
      </div>
      <Switch
        checked={launchAtLogin}
        onCheckedChange={(v) => updateSettings({ launchAtLogin: v })}
      />
    </div>

    <div class="p-4 flex items-start justify-between gap-4">
      <div class="flex items-start gap-3 min-w-0">
        <EyeOff class="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
        <div class="min-w-0">
          <Label>{t('wizard.background.launch_minimized')}</Label>
          <p class="text-xs text-muted-foreground mt-1">
            {t('wizard.background.launch_minimized_helper')}
          </p>
        </div>
      </div>
      <Switch
        checked={launchMinimized}
        onCheckedChange={(v) => updateSettings({ launchMinimized: v })}
      />
    </div>

    <div class="p-4 flex items-start justify-between gap-4">
      <div class="flex items-start gap-3 min-w-0">
        <MonitorOff class="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
        <div class="min-w-0">
          <Label>{t('wizard.background.stay_running_on_window_close')}</Label>
          <p class="text-xs text-muted-foreground mt-1">
            {t('wizard.background.stay_running_on_window_close_helper')}
          </p>
        </div>
      </div>
      <Switch
        checked={stayRunningOnWindowClose}
        onCheckedChange={(v) => updateSettings({ stayRunningOnWindowClose: v })}
      />
    </div>
  </div>
</div>
