<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Select from '$lib/components/ui/select';
  import { Label } from '$lib/components/ui/label';
  import { Mic, Square, Check } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status, type InputDeviceEntry } from '$lib/tauri';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { audioLevel } from '$lib/stores/recording';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let devices = $state<InputDeviceEntry[]>([]);
  let selectedDevice = $state<string | null>(null);

  // Live diagnostics during the test.
  let currentPeak = $state(0); // running max of $audioLevel during this test
  let testStartedAt = $state(0); // performance.now() at start
  let hint = $state<'try_other' | 'speak_louder' | null>(null);
  let hintTimer: ReturnType<typeof setInterval> | null = null;

  // Bar history for the inline waveform (24 bars).
  const BARS = 24;
  let bars = $state<number[]>(Array(BARS).fill(0));

  type Result =
    | { kind: 'ok'; peak: number; device: string }
    | { kind: 'no_capture' }
    | { kind: 'cancelled' }
    | { kind: 'no_audio'; peak: number; device: string }
    | { kind: 'error'; message: string };

  let result = $state<Result | null>(null);

  async function refreshPermission() {
    status = await lda.checkMicrophone();
  }

  async function refreshDevices() {
    try {
      devices = await lda.listInputDevices();
    } catch (e) {
      console.warn('listInputDevices failed', e);
    }
  }

  onMount(async () => {
    await Promise.all([refreshPermission(), refreshDevices()]);
    if ($settings) selectedDevice = $settings.inputDeviceName ?? null;
  });

  onDestroy(() => {
    if (hintTimer) clearInterval(hintTimer);
    if (testing) void lda.cancelTestMic();
  });

  // Push audio levels into the bar history + track the running peak while
  // testing. ONLY $audioLevel is tracked (the trigger); everything else is
  // wrapped in untrack so we never read state we also write — otherwise
  // Svelte 5 reports effect_update_depth_exceeded.
  $effect(() => {
    const level = $audioLevel;
    untrack(() => {
      if (!testing) return;
      if (level > currentPeak) currentPeak = level;
      bars = [...bars.slice(1), level];
    });
  });

  async function startTest() {
    testing = true;
    result = null;
    currentPeak = 0;
    hint = null;
    bars = Array(BARS).fill(0);
    testStartedAt = performance.now();

    // Smart hint scheduler.
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
      if (res.cancelled) {
        result = { kind: 'cancelled' };
      } else if (res.sampleCount === 0) {
        result = { kind: 'no_capture' };
      } else if (res.detected) {
        result = { kind: 'ok', peak: res.peak, device: res.deviceLabel };
      } else {
        result = { kind: 'no_audio', peak: res.peak, device: res.deviceLabel };
      }
    } catch (e) {
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
    try {
      await lda.cancelTestMic();
      console.info('[stopTest] cancellation dispatched');
    } catch (e) {
      console.error('[stopTest] cancel_test_mic invoke failed', e);
    }
  }

  async function selectDevice(name: string | null) {
    selectedDevice = name;
    await updateSettings({ inputDeviceName: name });
    result = null;
    hint = null;
  }

  // Display name of the currently-selected device for the dropdown trigger.
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

  <!-- Test card -->
  <div class="w-full bg-surface border border-border rounded-2xl p-5 space-y-4">
    <!-- Live waveform area (only meaningful during a test) -->
    <div class="h-16 flex items-end justify-center gap-[3px]" aria-hidden="true">
      {#each bars as level, i (i)}
        <div
          class="w-[4px] rounded-full transition-all duration-75
            {testing ? (level >= 0.02 ? 'bg-success' : 'bg-primary/60') : 'bg-muted'}"
          style="height: {Math.max(3, level * 56)}px"
        ></div>
      {/each}
    </div>

    <!-- Status text + peak readout -->
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
      {:else if result?.kind === 'cancelled'}
        <p class="text-muted-foreground">{t('wizard.microphone.tested_cancelled')}</p>
      {:else if result?.kind === 'error'}
        <div class="space-y-1">
          <p class="font-medium text-destructive">{t('wizard.microphone.tested_error')}</p>
          <p class="text-xs text-muted-foreground font-mono">{result.message}</p>
        </div>
      {:else}
        <p class="text-muted-foreground">{t('wizard.microphone.idle_hint')}</p>
      {/if}
    </div>

    <!-- Action button -->
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

  <!-- Device picker (always visible, even during testing — selection takes effect next time) -->
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
