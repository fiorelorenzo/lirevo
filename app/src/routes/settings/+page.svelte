<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import * as Select from '$lib/components/ui/select';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Switch } from '$lib/components/ui/switch';
  import { Slider } from '$lib/components/ui/slider';
  import { Separator } from '$lib/components/ui/separator';
  import FilePicker from '$lib/components/FilePicker.svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';
  import { lda, type Hotkey, type InputDeviceEntry } from '$lib/tauri';
  import { showToast } from '$lib/stores/toasts';
  import { onMount } from 'svelte';

  type Tab = 'general' | 'models' | 'hotkey' | 'about';
  let activeTab: Tab = $state('general');
  let checkingUpdates = $state(false);

  const HOTKEY_OPTIONS: { value: Hotkey; label: string }[] = [
    { value: 'right-option', label: 'Right Option ⌥' },
    { value: 'left-option', label: 'Left Option ⌥' },
    { value: 'right-command', label: 'Right Command ⌘' },
    { value: 'fn', label: 'Fn / Globe' },
    { value: 'f5', label: 'F5' },
  ];

  const LANGUAGE_OPTIONS = [
    { value: 'auto', label: t('settings.general.language_auto') },
    { value: 'en', label: 'English' },
    { value: 'it', label: 'Italiano' },
    { value: 'fr', label: 'Français' },
    { value: 'de', label: 'Deutsch' },
    { value: 'es', label: 'Español' },
  ];

  const TABS: Tab[] = ['general', 'models', 'hotkey', 'about'];

  let devices = $state<InputDeviceEntry[]>([]);
  onMount(async () => {
    try {
      devices = await lda.listInputDevices();
    } catch {
      // not fatal — keep dropdown empty
    }
  });

  async function checkUpdates() {
    checkingUpdates = true;
    try {
      const info = await lda.checkForUpdates();
      showToast('info', info.available ? `Update available: ${info.version}` : 'You are on the latest version.');
    } catch (e) {
      showToast('error', `Update check failed: ${e}`);
    } finally {
      checkingUpdates = false;
    }
  }
</script>

<div class="h-full flex">
  <!-- Sidebar -->
  <nav class="w-44 bg-muted/30 backdrop-blur-xl border-r border-border p-3 flex flex-col gap-1">
    <button
      class="text-left px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors mb-2"
      onclick={() => navigate('home')}
    >
      ← {t('settings.back_to_home')}
    </button>
    {#each TABS as tab (tab)}
      <button
        class={[
          'text-left px-3 py-2 rounded-md text-sm transition-colors',
          activeTab === tab ? 'bg-primary text-primary-foreground' : 'hover:bg-accent',
        ].join(' ')}
        onclick={() => (activeTab = tab)}
      >
        {t(`settings.tabs.${tab}`)}
      </button>
    {/each}
  </nav>

  <!-- Content -->
  <section class="flex-1 p-8 overflow-y-auto">
    {#if $settings && activeTab === 'general'}
      <h2 class="text-xl font-semibold mb-6">{t('settings.general.section')}</h2>

      <div class="space-y-6 max-w-lg">
        <div class="space-y-2">
          <Label>{t('settings.general.language')}</Label>
          <Select.Root
            type="single"
            value={$settings.language}
            onValueChange={(v) => v && updateSettings({ language: v })}
          >
            <Select.Trigger class="w-full">
              {LANGUAGE_OPTIONS.find((o) => o.value === $settings.language)?.label ?? $settings.language}
            </Select.Trigger>
            <Select.Content>
              {#each LANGUAGE_OPTIONS as opt (opt.value)}
                <Select.Item value={opt.value}>{opt.label}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>

        <div class="space-y-2">
          <Label>{t('settings.general.input_device')}</Label>
          <Select.Root
            type="single"
            value={$settings.inputDeviceName ?? '__default__'}
            onValueChange={(v) =>
              updateSettings({ inputDeviceName: v === '__default__' ? null : (v ?? null) })}
            disabled={devices.length === 0}
          >
            <Select.Trigger class="w-full">
              {$settings.inputDeviceName
                ?? (devices.find((d) => d.isDefault)?.name
                    ? `${devices.find((d) => d.isDefault)?.name} (${t('settings.general.input_device_default')})`
                    : t('settings.general.input_device_default'))}
            </Select.Trigger>
            <Select.Content>
              <Select.Item value="__default__">
                {devices.find((d) => d.isDefault)?.name
                  ? `${devices.find((d) => d.isDefault)?.name} (${t('settings.general.input_device_default')})`
                  : t('settings.general.input_device_default')}
              </Select.Item>
              {#each devices as d (d.name)}
                <Select.Item value={d.name}>
                  {d.name}{d.isDefault ? ` (${t('settings.general.input_device_default')})` : ''}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>

        <div class="flex items-start gap-3">
          <Switch
            checked={$settings.forcePasteboard}
            onCheckedChange={(v) => updateSettings({ forcePasteboard: v })}
          />
          <div>
            <Label>{t('settings.general.force_pasteboard')}</Label>
            <p class="text-xs text-muted-foreground mt-1">{t('settings.general.force_pasteboard_helper')}</p>
          </div>
        </div>

        <div class="space-y-2">
          <Label>{t('settings.general.paste_delay_ms')}</Label>
          <Slider
            type="single"
            min={0}
            max={2000}
            step={10}
            value={$settings.pasteDelayMs}
            onValueChange={(v) => updateSettings({ pasteDelayMs: typeof v === 'number' ? v : v[0] })}
          />
          <div class="text-xs text-muted-foreground tabular-nums">{$settings.pasteDelayMs} ms</div>
        </div>

        <div class="flex items-center gap-3">
          <Switch
            checked={$settings.launchAtLogin}
            onCheckedChange={(v) => updateSettings({ launchAtLogin: v })}
          />
          <Label>{t('settings.general.launch_at_login')}</Label>
        </div>
      </div>

    {:else if $settings && activeTab === 'models'}
      <h2 class="text-xl font-semibold mb-6">{t('settings.tabs.models')}</h2>

      <div class="space-y-6 max-w-lg">
        <div class="space-y-2">
          <Label>{t('settings.models.whisper_model')}</Label>
          <FilePicker
            value={$settings.whisperModelPath}
            filters={[{ name: 'Whisper ggml', extensions: ['bin'] }]}
            onpick={(p) => updateSettings({ whisperModelPath: p })}
          />
        </div>

        <div class="space-y-2">
          <Label>{t('settings.models.llm_model')}</Label>
          <FilePicker
            value={$settings.llmModelPath}
            filters={[{ name: 'GGUF', extensions: ['gguf'] }]}
            onpick={(p) => updateSettings({ llmModelPath: p })}
          />
        </div>

        <div class="space-y-2">
          <Label>{t('settings.models.llm_ctx_size')}</Label>
          <Input
            type="number"
            value={String($settings.llmCtxSize)}
            onchange={(e) => {
              const n = Number((e.currentTarget as HTMLInputElement).value);
              if (!Number.isNaN(n) && n >= 512 && n <= 32768) {
                updateSettings({ llmCtxSize: n });
              }
            }}
          />
        </div>

        <div class="flex items-center gap-3">
          <Switch
            checked={$settings.whisperCoreMLDisable}
            onCheckedChange={(v) => updateSettings({ whisperCoreMLDisable: v })}
          />
          <Label>{t('settings.models.whisper_coreml_disable')}</Label>
        </div>

        <Separator />

        <Button variant="outline" onclick={() => navigate('model-manager')}>
          {t('settings.models.open_manager')}
        </Button>
      </div>

    {:else if $settings && activeTab === 'hotkey'}
      <h2 class="text-xl font-semibold mb-6">{t('settings.tabs.hotkey')}</h2>

      <RadioGroup.Root
        value={$settings.hotkey}
        onValueChange={(v) => v && updateSettings({ hotkey: v as Hotkey })}
      >
        <div class="space-y-3">
          {#each HOTKEY_OPTIONS as opt (opt.value)}
            <label class="flex items-center gap-3 cursor-pointer">
              <RadioGroup.Item value={opt.value} />
              <span>{opt.label}</span>
            </label>
          {/each}
        </div>
      </RadioGroup.Root>

    {:else if $settings && activeTab === 'about'}
      <h2 class="text-xl font-semibold mb-6">{t('settings.tabs.about')}</h2>

      <div class="space-y-4 max-w-lg">
        <div class="p-4 bg-muted/40 rounded-lg space-y-1">
          <div class="font-semibold">local-dictation-app</div>
          <div class="text-sm text-muted-foreground tabular-nums">
            {t('settings.about.version')}: {$settings.appVersion}
          </div>
          <div class="text-sm text-muted-foreground">macOS · arm64</div>
        </div>

        <Button variant="outline" onclick={checkUpdates} disabled={checkingUpdates}>
          {checkingUpdates ? t('settings.about.checking') : t('settings.about.check_updates')}
        </Button>

        <Separator />

        <Button variant="outline" onclick={() => navigate('wizard')}>
          {t('settings.about.rerun_wizard')}
        </Button>

        <p class="text-xs text-muted-foreground pt-4">
          {t('settings.about.license')}
        </p>
      </div>
    {/if}
  </section>
</div>
