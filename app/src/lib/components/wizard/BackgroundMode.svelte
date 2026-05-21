<script lang="ts">
  // Wizard step: surface the three "how should this app live on your Mac"
  // toggles that otherwise hide in Settings → General → App. New users
  // don't go looking in Settings during their first session, so without
  // this step they'd never know the menu-bar-first experience exists.
  //
  // All three settings already have sensible defaults in `settings.rs`
  // (launch at login OFF, launch minimized OFF, stay running on close
  // ON). The wizard just makes them explicit choices.
  //
  // `keep_models_warm` is intentionally NOT in this step — it's a
  // performance preference, not a presence one, and the default is
  // already "on" which is what 99% of users want. Lives in Settings.
  import { Button } from '$lib/components/ui/button';
  import { Label } from '$lib/components/ui/label';
  import { Switch } from '$lib/components/ui/switch';
  import { Rocket, EyeOff, MonitorOff } from '@lucide/svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  // Local mirrors so toggles feel instant; updateSettings happens on
  // change so the persisted state matches the UI even if the user
  // bails out of the wizard via the Skip button.
  let launchAtLogin = $derived($settings?.launchAtLogin ?? false);
  let launchMinimized = $derived($settings?.launchMinimized ?? false);
  let stayRunningOnWindowClose = $derived(
    $settings?.stayRunningOnWindowClose ?? true,
  );
</script>

<div class="min-h-full flex flex-col items-center justify-center max-w-md mx-auto gap-6 text-center">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.background.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.background.body')}</p>

  <div class="w-full rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden text-left">
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

  <Button onclick={onnext}>{t('wizard.common.next')}</Button>
</div>
