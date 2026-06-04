<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { Separator } from '$lib/components/ui/separator';
  import * as Select from '$lib/components/ui/select';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Switch } from '$lib/components/ui/switch';
  import { Slider } from '$lib/components/ui/slider';
  import FilePicker from '$lib/components/FilePicker.svelte';
  import MicTest from '$lib/components/MicTest.svelte';
  import ModelCard from '$lib/components/ModelCard.svelte';
  import SkeletonRow from '$lib/components/SkeletonRow.svelte';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { profile, setProfileMode } from '$lib/stores/profile';
  import type { ProfileName } from '$lib/tauri';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';
  import {
    lda,
    type CatalogEntry,
    type Hotkey,
    type InputDeviceEntry,
    type LocalModel,
  } from '$lib/tauri';
  import {
    STT_MODELS,
    defaultModelId,
    formatSize as fmtSttSize,
  } from '$lib/models/catalog';
  import { Check, Sparkles } from '@lucide/svelte';
  import { toastInfo, toastError, withErrorToast } from '$lib/stores/toasts';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { page } from '$app/state';
  import { onDestroy, onMount } from 'svelte';

  type Tab = 'general' | 'models' | 'hotkey' | 'about';
  const TABS: Tab[] = ['general', 'models', 'hotkey', 'about'];

  function isTab(v: string | null): v is Tab {
    return v === 'general' || v === 'models' || v === 'hotkey' || v === 'about';
  }

  const initialTab = page.url.searchParams.get('tab');
  let activeTab: Tab = $state(isTab(initialTab) ? initialTab : 'general');
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

  const ENERGY_OPTIONS = [
    { value: 'auto', label: t('settings.general.energy_auto') },
    { value: 'power_saver', label: t('settings.general.energy_power_saver') },
    { value: 'balanced', label: t('settings.general.energy_balanced') },
    { value: 'performance', label: t('settings.general.energy_performance') },
  ];

  // The live store's `mode` wins once it resolves; before that fall back to
  // the persisted choice so the control shows the right value on first paint.
  let energyMode = $derived($profile?.mode ?? $settings?.profileMode ?? 'auto');
  const PROFILE_LABELS: Record<ProfileName, string> = {
    powerSaver: 'Power Saver',
    balanced: 'Balanced',
    performance: 'Performance',
  };
  // When pinned to Auto, surface which concrete profile is currently active.
  let resolvedActive = $derived(
    energyMode === 'auto' && $profile?.active
      ? t('settings.general.energy_auto_active', { profile: PROFILE_LABELS[$profile.active] })
      : null,
  );

  let devices = $state<InputDeviceEntry[]>([]);

  // Model-tab state: catalog + locally installed models + download events.
  let catalog = $state<CatalogEntry[]>([]);
  let local = $state<LocalModel[]>([]);
  let modelsLoaded = $state(false);
  let unlistenDownload: UnlistenFn | null = null;

  async function refreshModels() {
    const result = await withErrorToast(t('settings.models.error.refresh'), () =>
      Promise.all([lda.modelsCatalog(), lda.modelsListLocal()]),
    );
    if (result !== null) {
      [catalog, local] = result;
    }
    modelsLoaded = true;
  }

  onMount(async () => {
    // Only enumerate input devices if mic permission was already granted.
    // On macOS 14+ Core Audio HAL surfaces the TCC prompt the moment we
    // open the device list, even read-only — we don't want to flash that
    // dialog every time the user opens Settings.
    const mic = await lda.checkMicrophone().catch(() => 'denied' as const);
    if (mic === 'granted') {
      try {
        devices = await lda.listInputDevices();
      } catch {
        // not fatal — keep dropdown empty
      }
    }

    await refreshModels();
    unlistenDownload = await lda.onDownloadProgress(async (p) => {
      if (p.state === 'complete') {
        await refreshModels();
        const entry = catalog.find((c) => c.id === p.id);
        const localMatch = local.find((l) => l.id === p.id);
        if (entry && localMatch) {
          const patch = entry.kind === 'stt'
            ? { whisperModelPath: localMatch.path }
            : { llmModelPath: localMatch.path };
          await updateSettings(patch);
        }
      }
    });
  });

  onDestroy(() => {
    unlistenDownload?.();
  });

  function installed(id: string): boolean {
    return local.some((l) => l.id === id);
  }

  function selectedFor(kind: 'stt' | 'llm'): string | null {
    if (!$settings) return null;
    return kind === 'stt' ? $settings.whisperModelPath : $settings.llmModelPath;
  }

  function selectModel(entry: CatalogEntry) {
    const match = local.find((l) => l.id === entry.id);
    if (!match) return;
    const patch = entry.kind === 'stt'
      ? { whisperModelPath: match.path }
      : { llmModelPath: match.path };
    void updateSettings(patch);
  }

  function fmtSize(bytes: number): string {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }

  let usedBytes = $derived(local.reduce((s, l) => s + l.sizeBytes, 0));
  let installedCount = $derived(local.filter((l) => l.inCatalog).length);
  // M4: STT moved to the audiopipe-backed catalog (see $lib/models/catalog).
  // The legacy `KINDS` loop now only renders the LLM half; STT has its own
  // dedicated section above it.
  const KINDS: ('stt' | 'llm')[] = ['llm'];

  // M4 STT section state.
  // Active = whatever `sttModelId` currently resolves to. When the field
  // is null the backend falls back to defaultModelId() — mirror that here
  // so the "In use" badge always lands somewhere.
  let activeSttId = $derived($settings?.sttModelId ?? defaultModelId());
  // Track which model we're currently hot-swapping to, so the user gets
  // immediate feedback ("Switching…") between the click and the next
  // model-state event landing.
  let switchingTo = $state<string | null>(null);

  async function useSttModel(id: string) {
    if (id === activeSttId || switchingTo === id) return;
    switchingTo = id;
    toastInfo(t('settings.models.switch_toast'));
    const result = await updateSettings({ sttModelId: id });
    if (result === null) {
      // updateSettings already toasted the error; clear the spinner.
      switchingTo = null;
      return;
    }
    // The backend reloads asynchronously; the model-state listener (held
    // by app code elsewhere) will surface progress. Clearing the local
    // spinner state once the settings round-trip lands is good enough —
    // the "In use" badge below derives from activeSttId, which already
    // updated when updateSettings resolved.
    switchingTo = null;
  }

  async function checkUpdates() {
    checkingUpdates = true;
    try {
      const info = await lda.checkForUpdates();
      toastInfo(info.available ? `Update available: ${info.version}` : 'You are on the latest version.');
    } catch (e) {
      toastError(`Update check failed: ${e}`);
    } finally {
      checkingUpdates = false;
    }
  }
</script>

<div class="h-full flex">
  <!-- Sidebar -->
  <nav class="w-44 bg-muted/30 backdrop-blur-xl border-r border-border p-3 flex flex-col gap-1">
    <!-- Passive top strip so the settings window can be dragged from the
         sidebar header, like a native macOS toolbar. -->
    <div data-tauri-drag-region class="h-6 -mx-3 -mt-3 mb-1 pointer-events-none"></div>
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
      <div class="space-y-8 max-w-lg">
        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t('settings.general.section')}
          </h2>
          <div class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden">
            <div class="p-4 flex items-center justify-between gap-4">
              <Label class="shrink-0">{t('settings.general.language')}</Label>
              <Select.Root
                type="single"
                value={$settings.language}
                onValueChange={(v) => v && updateSettings({ language: v })}
              >
                <Select.Trigger class="w-56">
                  <span class="flex-1 min-w-0 truncate text-left">
                    {LANGUAGE_OPTIONS.find((o) => o.value === $settings.language)?.label ?? $settings.language}
                  </span>
                </Select.Trigger>
                <Select.Content>
                  {#each LANGUAGE_OPTIONS as opt (opt.value)}
                    <Select.Item value={opt.value}>{opt.label}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </div>

            <div class="p-4 flex items-center justify-between gap-4">
              <Label class="shrink-0">{t('settings.general.input_device')}</Label>
              <Select.Root
                type="single"
                value={$settings.inputDeviceName ?? '__default__'}
                onValueChange={(v) =>
                  updateSettings({ inputDeviceName: v === '__default__' ? null : (v ?? null) })}
                disabled={devices.length === 0}
              >
                <Select.Trigger class="w-56">
                  <span class="flex-1 min-w-0 truncate text-left">
                    {$settings.inputDeviceName
                      ?? (devices.find((d) => d.isDefault)?.name
                          ? `${devices.find((d) => d.isDefault)?.name} (${t('settings.general.input_device_default')})`
                          : t('settings.general.input_device_default'))}
                  </span>
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
          </div>
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-1">
            {t('settings.general.microphone.section')}
          </h2>
          <p class="text-xs text-muted-foreground mb-3">
            {t('settings.general.microphone.section_helper')}
          </p>
          <MicTest />
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t('settings.general.injection_section')}
          </h2>
          <div class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden">
            <div class="p-4 flex items-start justify-between gap-4">
              <div class="min-w-0">
                <Label>{t('settings.general.force_pasteboard')}</Label>
                <p class="text-xs text-muted-foreground mt-1">{t('settings.general.force_pasteboard_helper')}</p>
              </div>
              <Switch
                checked={$settings.forcePasteboard}
                onCheckedChange={(v) => updateSettings({ forcePasteboard: v })}
              />
            </div>
            <div class="p-4 space-y-3">
              <div class="flex items-center justify-between gap-4">
                <Label>{t('settings.general.paste_delay_ms')}</Label>
                <span class="text-xs text-muted-foreground tabular-nums">{$settings.pasteDelayMs} ms</span>
              </div>
              <Slider
                type="single"
                min={0}
                max={2000}
                step={10}
                value={$settings.pasteDelayMs}
                onValueChange={(v) => updateSettings({ pasteDelayMs: typeof v === 'number' ? v : v[0] })}
              />
            </div>
          </div>
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t('settings.general.app_section')}
          </h2>
          <div class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden">
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t('settings.general.launch_at_login')}</Label>
              </div>
              <Switch
                checked={$settings.launchAtLogin}
                onCheckedChange={(v) => updateSettings({ launchAtLogin: v })}
              />
            </div>
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t('settings.general.record_history')}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t('settings.general.record_history_helper')}
                </p>
              </div>
              <Switch
                checked={$settings.recordHistory}
                onCheckedChange={(v) => updateSettings({ recordHistory: v })}
              />
            </div>
            <div class="p-4 flex items-start justify-between gap-4">
              <div class="min-w-0">
                <Label>{t('settings.general.energy')}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t('settings.general.energy_helper')}
                </p>
              </div>
              <div class="shrink-0 flex flex-col items-end gap-1.5">
                <Select.Root
                  type="single"
                  value={energyMode}
                  onValueChange={(v) => v && setProfileMode(v)}
                >
                  <Select.Trigger class="w-40">
                    <span class="flex-1 min-w-0 truncate text-left">
                      {ENERGY_OPTIONS.find((o) => o.value === energyMode)?.label ?? energyMode}
                    </span>
                  </Select.Trigger>
                  <Select.Content>
                    {#each ENERGY_OPTIONS as opt (opt.value)}
                      <Select.Item value={opt.value}>{opt.label}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
                {#if resolvedActive}
                  <span class="text-xs text-muted-foreground">{resolvedActive}</span>
                {/if}
              </div>
            </div>
          </div>
        </section>
      </div>

    {:else if $settings && activeTab === 'models'}
      <div class="space-y-8 max-w-2xl">
        {#if modelsLoaded}
          <div class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-muted/50 text-xs text-muted-foreground">
            {t('settings.models.stats', { used: fmtSize(usedBytes), installed: installedCount, total: catalog.length })}
          </div>

          <!-- M4 STT section: hot-swappable audiopipe models. Source of truth
               is $lib/models/catalog; the legacy catalog (`local`,
               `installed`) only governs LLM rows now. -->
          <section>
            <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-1">
              {t('settings.models.stt_section')}
            </h2>
            <p class="text-xs text-muted-foreground mb-3">
              {t('settings.models.stt_section_helper')}
            </p>
            <div class="space-y-2">
              {#each STT_MODELS as entry (entry.id)}
                {@const isActive = entry.id === activeSttId}
                {@const isSwitching = switchingTo === entry.id}
                <div
                  class={[
                    'w-full p-4 bg-surface border-2 rounded-lg transition-colors duration-150',
                    isActive ? 'border-primary ring-2 ring-primary/30' : 'border-border',
                  ].join(' ')}
                >
                  <div class="flex items-start gap-4">
                    <div class="flex-1 min-w-0">
                      <div class="flex items-baseline gap-2 flex-wrap">
                        <span class="font-medium">{entry.displayName}</span>
                        <span class="text-xs text-muted-foreground tabular-nums">
                          {fmtSttSize(entry.sizeBytes)}
                        </span>
                        {#if entry.default}
                          <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[11px] font-medium leading-none">
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
                    <div class="shrink-0 flex items-center gap-2">
                      {#if isActive}
                        <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 text-primary text-xs font-medium">
                          <Check class="h-3 w-3" />
                          {t('settings.models.in_use_badge')}
                        </span>
                      {:else if isSwitching}
                        <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-muted text-muted-foreground text-xs font-medium">
                          {t('settings.models.switching_badge')}
                        </span>
                      {:else}
                        <Button variant="outline" size="sm" onclick={() => useSttModel(entry.id)}>
                          {t('settings.models.use_button')}
                        </Button>
                      {/if}
                    </div>
                  </div>
                </div>
              {/each}
            </div>
            <Separator class="mt-6" />
          </section>

          {#each KINDS as kind, i (kind)}
            <section>
              <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
                {kind === 'stt' ? t('wizard.models.stt_section') : t('wizard.models.llm_section')}
              </h2>

              <div class="space-y-2">
                {#each catalog
                  .filter((c) => c.kind === kind)
                  .toSorted((a, b) => {
                    if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
                    const sa = a.scores?.compositeWeighted ?? -1;
                    const sb = b.scores?.compositeWeighted ?? -1;
                    if (sa !== sb) return sb - sa;
                    return b.sizeBytes - a.sizeBytes;
                  }) as entry (entry.id)}
                  <ModelCard
                    {entry}
                    installed={installed(entry.id)}
                    selected={selectedFor(kind) === local.find((l) => l.id === entry.id)?.path}
                    onselect={() => selectModel(entry)}
                    ondelete={refreshModels}
                  />
                {/each}
              </div>

              <div class="text-xs uppercase tracking-wide text-muted-foreground mt-4 mb-2">
                {t('wizard.models.use_existing')}
              </div>
              <FilePicker
                value={selectedFor(kind)}
                filters={kind === 'stt'
                  ? [{ name: 'Whisper ggml', extensions: ['bin'] }]
                  : [{ name: 'GGUF', extensions: ['gguf'] }]}
                onpick={(p) => updateSettings(kind === 'stt' ? { whisperModelPath: p } : { llmModelPath: p })}
              />

              {#if i < KINDS.length - 1}
                <Separator class="mt-6" />
              {/if}
            </section>
          {/each}

          <section>
            <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
              {t('settings.models.advanced_section')}
            </h2>
            <div class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden">
              <div class="p-4 flex items-center justify-between gap-4">
                <Label>{t('settings.models.whisper_coreml_disable')}</Label>
                <Switch
                  checked={$settings.whisperCoreMLDisable}
                  onCheckedChange={(v) => updateSettings({ whisperCoreMLDisable: v })}
                />
              </div>
              <div class="p-4 flex items-center justify-between gap-4">
                <Label>{t('settings.models.llm_ctx_size')}</Label>
                <Input
                  type="number"
                  class="w-32"
                  value={String($settings.llmCtxSize)}
                  onchange={(e) => {
                    const n = Number((e.currentTarget as HTMLInputElement).value);
                    if (!Number.isNaN(n) && n >= 512 && n <= 32768) {
                      updateSettings({ llmCtxSize: n });
                    }
                  }}
                />
              </div>
            </div>
          </section>
        {:else}
          <div class="space-y-3">
            <SkeletonRow class="h-4 w-32" />
            <SkeletonRow class="h-16 w-full" />
            <SkeletonRow class="h-16 w-full" />
            <SkeletonRow class="h-16 w-full" />
          </div>
        {/if}
      </div>

    {:else if $settings && activeTab === 'hotkey'}
      <div class="space-y-3 max-w-lg">
        <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
          {t('settings.hotkey.label')}
        </h2>
        <div class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden">
          <RadioGroup.Root
            value={$settings.hotkey}
            onValueChange={(v) => v && updateSettings({ hotkey: v as Hotkey })}
          >
            {#each HOTKEY_OPTIONS as opt (opt.value)}
              <label class="flex items-center gap-3 p-4 cursor-pointer hover:bg-accent/40 transition-colors border-b border-border last:border-b-0">
                <RadioGroup.Item value={opt.value} />
                <span class="text-sm">{opt.label}</span>
              </label>
            {/each}
          </RadioGroup.Root>
        </div>
      </div>

    {:else if $settings && activeTab === 'about'}
      <div class="space-y-6 max-w-lg">
        <div class="rounded-xl border border-border bg-surface p-5 space-y-1">
          <div class="font-semibold text-lg">Lirevo</div>
          <div class="text-sm text-muted-foreground tabular-nums">
            {t('settings.about.version')}: {$settings.appVersion}
          </div>
          <div class="text-sm text-muted-foreground">macOS · arm64</div>
        </div>

        <div class="flex flex-wrap gap-3">
          <Button variant="outline" onclick={checkUpdates} disabled={checkingUpdates}>
            {checkingUpdates ? t('settings.about.checking') : t('settings.about.check_updates')}
          </Button>
          <Button variant="outline" onclick={() => navigate('wizard')}>
            {t('settings.about.rerun_wizard')}
          </Button>
        </div>

        <p class="text-xs text-muted-foreground">
          {t('settings.about.license')}
        </p>
      </div>
    {/if}
  </section>
</div>
