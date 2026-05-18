<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Select from '$lib/components/ui/select';
  import { Label } from '$lib/components/ui/label';
  import { Mic, Square, Check } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status, type InputDeviceEntry } from '$lib/tauri';

  async function openMicrophoneSettings() {
    console.info('[Microphone] open System Settings clicked');
    try {
      await lda.openSystemSettingsMicrophone();
    } catch (e) {
      console.error('[Microphone] open settings failed', e);
    }
  }
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { audioLevel } from '$lib/stores/recording';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let devices = $state<InputDeviceEntry[]>([]);
  let selectedDevice = $state<string | null>(null);

  let currentPeak = $state(0);
  let testStartedAt = $state(0);
  let hint = $state<'try_other' | 'speak_louder' | null>(null);
  let hintTimer: ReturnType<typeof setInterval> | null = null;

  const BARS = 24;
  let bars = $state<number[]>(Array(BARS).fill(0));

  // We mutate barsBuf imperatively (NOT $state) so we don't establish any
  // self-referential write inside the audioLevel subscription. After mutating
  // we copy into `bars` (the reactive view consumed by {#each}).
  let barsBuf: number[] = Array(BARS).fill(0);

  // Imperative store subscription — bypasses $effect tracking entirely so we
  // can't trip Svelte 5's effect_update_depth_exceeded guard.
  let unsubAudioLevel: (() => void) | null = null;

  type Result =
    | { kind: 'ok'; peak: number; device: string }
    | { kind: 'no_capture' }
    | { kind: 'device_silent'; device: string }
    | { kind: 'cancelled' }
    | { kind: 'no_audio'; peak: number; device: string }
    | { kind: 'tcc_blocked' }
    | { kind: 'error'; message: string };

  let result = $state<Result | null>(null);

  async function refreshPermission() {
    status = await lda.checkMicrophone();
    console.info(`[Microphone] permission status = ${status}`);
  }

  async function refreshDevices() {
    try {
      devices = await lda.listInputDevices();
      console.info(`[Microphone] devices = ${JSON.stringify(devices)}`);
    } catch (e) {
      console.warn('[Microphone] listInputDevices failed', e);
    }
  }

  onMount(async () => {
    console.info('[Microphone] mount');
    await Promise.all([refreshPermission(), refreshDevices()]);
    if ($settings) selectedDevice = $settings.inputDeviceName ?? null;

    // Subscribe to audio levels imperatively.
    unsubAudioLevel = audioLevel.subscribe((level) => {
      if (!testing) return;
      barsBuf = [...barsBuf.slice(1), level];
      bars = barsBuf.slice(); // copy into reactive state
      if (level > currentPeak) currentPeak = level;
    });
  });

  onDestroy(() => {
    console.info('[Microphone] destroy');
    unsubAudioLevel?.();
    if (hintTimer) clearInterval(hintTimer);
    if (testing) void lda.cancelTestMic();
  });

  async function startTest() {
    console.info(`[Microphone] startTest device=${selectedDevice ?? '(default)'}`);

    // If TCC was never asked, prompt explicitly first. cpal on macOS opens
    // the device through Core Audio HAL which does NOT trigger the TCC
    // prompt automatically when run from an unsigned dev build — the stream
    // succeeds but produces silent zeros.
    if (status === 'not_determined') {
      console.info('[Microphone] permission not_determined — prompting');
      const t0 = performance.now();
      try {
        status = await lda.promptMicrophone();
        console.info(
          `[Microphone] post-prompt status = ${status} (${(performance.now() - t0).toFixed(0)}ms)`,
        );
      } catch (e) {
        console.error('[Microphone] promptMicrophone failed', e);
      }
      if (status !== 'granted') {
        const elapsed = performance.now() - t0;
        // < 250ms means macOS auto-denied without showing the dialog
        // (typically because the responsible process — Terminal, parent
        // shell — doesn't have mic permission, or TCC remembers a prior
        // deny). Show a Settings-link path instead.
        if (elapsed < 250) {
          result = { kind: 'tcc_blocked' };
        } else {
          result = { kind: 'error', message: 'Microphone permission was not granted.' };
        }
        return;
      }
    }

    testing = true;
    result = null;
    currentPeak = 0;
    hint = null;
    barsBuf = Array(BARS).fill(0);
    bars = barsBuf.slice();
    testStartedAt = performance.now();

    hintTimer = setInterval(() => {
      const elapsedMs = performance.now() - testStartedAt;
      if (elapsedMs > 7000 && currentPeak > 0 && currentPeak < 0.02) {
        hint = 'speak_louder';
      } else if (elapsedMs > 3500 && currentPeak < 0.005) {
        hint = 'try_other';
      }
    }, 1000);

    try {
      const res = await lda.testMic(selectedDevice);
      console.info(`[Microphone] testMic resolved: ${JSON.stringify(res)}`);
      if (res.cancelled) {
        result = { kind: 'cancelled' };
      } else if (res.sampleCount === 0) {
        result = { kind: 'no_capture' };
      } else if (res.detected) {
        result = { kind: 'ok', peak: res.peak, device: res.deviceLabel };
      } else if (res.deviceSilent) {
        result = { kind: 'device_silent', device: res.deviceLabel };
      } else {
        result = { kind: 'no_audio', peak: res.peak, device: res.deviceLabel };
      }
    } catch (e) {
      console.error('[Microphone] testMic threw', e);
      result = { kind: 'error', message: String(e) };
    } finally {
      if (hintTimer) {
        clearInterval(hintTimer);
        hintTimer = null;
      }
      testing = false;
      await refreshPermission();
    }
  }

  async function stopTest() {
    console.info('[Microphone] stopTest clicked');
    try {
      await lda.cancelTestMic();
      console.info('[Microphone] cancelTestMic dispatched');
    } catch (e) {
      console.error('[Microphone] cancelTestMic failed', e);
    }
  }

  async function selectDevice(name: string | null) {
    console.info(`[Microphone] selectDevice ${name ?? '(default)'}`);
    selectedDevice = name;
    await updateSettings({ inputDeviceName: name });
    result = null;
    hint = null;
  }

  let triggerLabel = $derived.by(() => {
    if (selectedDevice) return selectedDevice;
    const def = devices.find((d) => d.isDefault);
    return def
      ? `${def.name} (${t('wizard.microphone.default')})`
      : t('wizard.microphone.default');
  });
</script>

<div class="flex flex-col items-center max-w-md mx-auto gap-5 text-center pb-2">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.microphone.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.microphone.body')}</p>

  <PermissionStatus
    {status}
    granted_label={t('wizard.microphone.granted')}
    denied_label={t('wizard.microphone.denied')}
    not_determined_label={t('wizard.microphone.not_determined')}
  />

  <div class="w-full bg-surface border border-border rounded-2xl p-5 space-y-4">
    <div class="h-16 flex items-end justify-center gap-[3px]" aria-hidden="true">
      {#each bars as level, i (i)}
        <div
          class="w-[4px] rounded-full transition-all duration-75
            {testing ? (level >= 0.02 ? 'bg-success' : 'bg-primary/60') : 'bg-muted'}"
          style="height: {Math.max(3, level * 56)}px"
        ></div>
      {/each}
    </div>

    <div class="text-sm">
      {#if testing}
        <div class="flex items-center justify-center gap-2 text-muted-foreground">
          <span class="relative flex h-2 w-2">
            <span class="absolute inset-0 rounded-full bg-destructive animate-ping opacity-75"></span>
            <span class="relative inline-flex h-2 w-2 rounded-full bg-destructive"></span>
          </span>
          {t('wizard.microphone.listening')}
          <span class="text-xs text-muted-foreground tabular-nums">
            · peak {(currentPeak * 100).toFixed(0)}%
          </span>
        </div>
        {#if hint === 'try_other'}
          <p class="text-xs text-warning mt-2">{t('wizard.microphone.hint_try_other')}</p>
        {:else if hint === 'speak_louder'}
          <p class="text-xs text-warning mt-2">{t('wizard.microphone.hint_speak_louder')}</p>
        {/if}
      {:else if result?.kind === 'ok'}
        <div class="flex items-center justify-center gap-2 font-medium text-success">
          <Check class="h-4 w-4" />
          {t('wizard.microphone.tested_ok')}
          <span class="text-xs text-muted-foreground tabular-nums">
            · peak {(result.peak * 100).toFixed(0)}%
          </span>
        </div>
      {:else if result?.kind === 'no_audio'}
        <div class="space-y-1">
          <p class="font-medium text-warning">{t('wizard.microphone.tested_no_audio')}</p>
          <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_audio_hint')}</p>
        </div>
      {:else if result?.kind === 'no_capture'}
        <div class="space-y-1">
          <p class="font-medium text-destructive">{t('wizard.microphone.tested_no_capture')}</p>
          <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_capture_hint')}</p>
        </div>
      {:else if result?.kind === 'device_silent'}
        <div class="space-y-1">
          <p class="font-medium text-warning">{t('wizard.microphone.tested_device_silent')}</p>
          <p class="text-xs text-muted-foreground">
            {t('wizard.microphone.tested_device_silent_hint', { device: result.device })}
          </p>
        </div>
      {:else if result?.kind === 'cancelled'}
        <p class="text-muted-foreground">{t('wizard.microphone.tested_cancelled')}</p>
      {:else if result?.kind === 'tcc_blocked'}
        <div class="space-y-2">
          <p class="font-medium text-destructive">{t('wizard.microphone.tested_tcc_blocked')}</p>
          <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_tcc_blocked_hint')}</p>
          <Button variant="outline" size="sm" onclick={openMicrophoneSettings}>
            {t('wizard.microphone.open_system_settings')}
          </Button>
        </div>
      {:else if result?.kind === 'error'}
        <div class="space-y-1">
          <p class="font-medium text-destructive">{t('wizard.microphone.tested_error')}</p>
          <p class="text-xs text-muted-foreground font-mono">{result.message}</p>
        </div>
      {:else}
        <p class="text-muted-foreground">{t('wizard.microphone.idle_hint')}</p>
      {/if}
    </div>

    <div class="flex items-center justify-center">
      {#if testing}
        <Button variant="outline" onclick={stopTest}>
          <Square class="h-4 w-4 mr-2" />
          {t('wizard.microphone.stop')}
        </Button>
      {:else}
        <Button onclick={startTest} disabled={status === 'denied'}>
          <Mic class="h-4 w-4 mr-2" />
          {t('wizard.microphone.test_mic')}
        </Button>
      {/if}
    </div>
  </div>

  <div class="w-full max-w-xs space-y-2 text-left">
    <Label class="text-xs uppercase tracking-wide text-muted-foreground">
      {t('wizard.microphone.input_device')}
    </Label>
    <Select.Root
      type="single"
      value={selectedDevice ?? '__default__'}
      onValueChange={(v) => selectDevice(v === '__default__' ? null : (v ?? null))}
      disabled={testing || devices.length === 0}
    >
      <Select.Trigger class="w-full">{triggerLabel}</Select.Trigger>
      <Select.Content>
        <Select.Item value="__default__">
          {devices.find((d) => d.isDefault)?.name
            ? `${devices.find((d) => d.isDefault)?.name} (${t('wizard.microphone.default')})`
            : t('wizard.microphone.default')}
        </Select.Item>
        {#each devices as d (d.name)}
          <Select.Item value={d.name}>
            {d.name}{d.isDefault ? ` (${t('wizard.microphone.default')})` : ''}
          </Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  <Button disabled={status !== 'granted'} onclick={onnext}>
    {t('wizard.common.next')}
  </Button>
</div>
