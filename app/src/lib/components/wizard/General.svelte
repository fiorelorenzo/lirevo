<script lang="ts">
  // Final wizard step: the hotkey picker plus the "Launch at login" toggle,
  // merged from the old Hotkey + BackgroundMode steps. Finishing here
  // completes the wizard. (Lirevo is a menu-bar app: it has no Dock icon,
  // starts silently in the tray when auto-launched at login, and stays in the
  // tray when its window is closed — so there's nothing else to configure.)
  import HotkeyRecorder from '$lib/components/HotkeyRecorder.svelte';
  import { Label } from '$lib/components/ui/label';
  import { Switch } from '$lib/components/ui/switch';
  import { Rocket, Bluetooth } from '@lucide/svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import type { ActivationMode } from '$lib/hotkey';
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

  let hotkeySpec = $state($settings?.hotkey);
  let activationMode = $state<ActivationMode>($settings?.activationMode ?? 'hold');

  // Seed local hotkey state once the store loads, in case $settings was still
  // null at mount (otherwise the {#if hotkeySpec} guard hides the recorder
  // forever). Local editing afterwards is preserved — this only fires while
  // hotkeySpec is still unset.
  $effect(() => {
    if (hotkeySpec === undefined && $settings) {
      hotkeySpec = $settings.hotkey;
      activationMode = $settings.activationMode;
    }
  });

  // Local mirrors so toggles feel instant; updateSettings runs on change so
  // persisted state matches the UI even if the user bails via Skip.
  let launchAtLogin = $derived($settings?.launchAtLogin ?? false);
  let smartMicRouting = $derived($settings?.smartMicRouting ?? true);

  async function finish() {
    if (hotkeySpec) await updateSettings({ hotkey: hotkeySpec, activationMode });
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
    {#if hotkeySpec}
      <HotkeyRecorder
        spec={hotkeySpec}
        mode={activationMode}
        onchange={(n) => {
          hotkeySpec = n.hotkey;
          activationMode = n.activationMode;
        }}
      />
    {/if}
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
        <Bluetooth class="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
        <div class="min-w-0">
          <Label>{t('wizard.general.smart_mic_routing')}</Label>
          <p class="text-xs text-muted-foreground mt-1">
            {t('wizard.general.smart_mic_routing_helper')}
          </p>
        </div>
      </div>
      <Switch
        checked={smartMicRouting}
        onCheckedChange={(v) => updateSettings({ smartMicRouting: v })}
      />
    </div>
  </div>
</div>
