<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic, Square, Check } from '@lucide/svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { toastError, withErrorToast } from '$lib/stores/toasts';
  import { settings } from '$lib/stores/settings.svelte';
  import { audioLevel } from '$lib/stores/recording';
  import { t } from '$lib/i18n';

  let testing = $state(false);
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

  async function openMicrophoneSettings() {
    await withErrorToast(t('settings.general.microphone.error.open_settings'), () =>
      lda.openSystemSettingsMicrophone(),
    );
  }

  onMount(() => {
    unsubAudioLevel = audioLevel.subscribe((level) => {
      if (!testing) return;
      barsBuf = [...barsBuf.slice(1), level];
      bars = barsBuf.slice();
      if (level > currentPeak) currentPeak = level;
    });
  });

  onDestroy(() => {
    unsubAudioLevel?.();
    if (hintTimer) clearInterval(hintTimer);
    if (testing) void lda.cancelTestMic();
  });

  async function startTest() {
    const selectedDevice = $settings?.inputDeviceName ?? null;

    // If TCC was never asked, prompt explicitly first. cpal on macOS opens
    // the device through Core Audio HAL which does NOT trigger the TCC
    // prompt automatically when run from an unsigned dev build — the stream
    // succeeds but produces silent zeros.
    const initialStatus = await lda.checkMicrophone().catch(() => 'denied' as const);
    if (initialStatus === 'not_determined') {
      const t0 = performance.now();
      let postStatus: Status = initialStatus;
      try {
        postStatus = await lda.promptMicrophone();
      } catch (e) {
        const reason = e instanceof Error ? e.message : String(e);
        toastError(`${t('settings.general.microphone.error.prompt')}: ${reason}`);
      }
      if (postStatus !== 'granted') {
        const elapsed = performance.now() - t0;
        if (elapsed < 250) {
          result = { kind: 'tcc_blocked' };
        } else {
          result = { kind: 'error', message: 'Microphone permission was not granted.' };
        }
        return;
      }
    } else if (initialStatus === 'denied') {
      result = { kind: 'tcc_blocked' };
      return;
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
      result = { kind: 'error', message: String(e) };
    } finally {
      if (hintTimer) {
        clearInterval(hintTimer);
        hintTimer = null;
      }
      testing = false;
    }
  }

  async function stopTest() {
    await withErrorToast(t('settings.general.microphone.error.cancel_test'), () =>
      lda.cancelTestMic(),
    );
  }
</script>

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

  <div class="text-sm text-center">
    {#if testing}
      <div class="flex items-center justify-center gap-2 text-muted-foreground">
        <span class="relative flex h-2 w-2">
          <span class="absolute inset-0 rounded-full bg-destructive animate-ping opacity-75"></span>
          <span class="relative inline-flex h-2 w-2 rounded-full bg-destructive"></span>
        </span>
        {t('settings.general.microphone.listening')}
        <span class="text-xs text-muted-foreground tabular-nums">
          · peak {(currentPeak * 100).toFixed(0)}%
        </span>
      </div>
      {#if hint === 'try_other'}
        <p class="text-xs text-warning mt-2">{t('settings.general.microphone.hint_try_other')}</p>
      {:else if hint === 'speak_louder'}
        <p class="text-xs text-warning mt-2">{t('settings.general.microphone.hint_speak_louder')}</p>
      {/if}
    {:else if result?.kind === 'ok'}
      <div class="flex items-center justify-center gap-2 font-medium text-success">
        <Check class="h-4 w-4" />
        {t('settings.general.microphone.tested_ok')}
        <span class="text-xs text-muted-foreground tabular-nums">
          · peak {(result.peak * 100).toFixed(0)}%
        </span>
      </div>
    {:else if result?.kind === 'no_audio'}
      <div class="space-y-1">
        <p class="font-medium text-warning">{t('settings.general.microphone.tested_no_audio')}</p>
        <p class="text-xs text-muted-foreground">{t('settings.general.microphone.tested_no_audio_hint')}</p>
      </div>
    {:else if result?.kind === 'no_capture'}
      <div class="space-y-1">
        <p class="font-medium text-destructive">{t('settings.general.microphone.tested_no_capture')}</p>
        <p class="text-xs text-muted-foreground">{t('settings.general.microphone.tested_no_capture_hint')}</p>
      </div>
    {:else if result?.kind === 'device_silent'}
      <div class="space-y-1">
        <p class="font-medium text-warning">{t('settings.general.microphone.tested_device_silent')}</p>
        <p class="text-xs text-muted-foreground">
          {t('settings.general.microphone.tested_device_silent_hint', { device: result.device })}
        </p>
      </div>
    {:else if result?.kind === 'cancelled'}
      <p class="text-muted-foreground">{t('settings.general.microphone.tested_cancelled')}</p>
    {:else if result?.kind === 'tcc_blocked'}
      <div class="space-y-2">
        <p class="font-medium text-destructive">{t('settings.general.microphone.tested_tcc_blocked')}</p>
        <p class="text-xs text-muted-foreground">{t('settings.general.microphone.tested_tcc_blocked_hint')}</p>
        <Button variant="outline" size="sm" onclick={openMicrophoneSettings}>
          {t('settings.general.microphone.open_system_settings')}
        </Button>
      </div>
    {:else if result?.kind === 'error'}
      <div class="space-y-1">
        <p class="font-medium text-destructive">{t('settings.general.microphone.tested_error')}</p>
        <p class="text-xs text-muted-foreground font-mono">{result.message}</p>
      </div>
    {:else}
      <p class="text-muted-foreground">{t('settings.general.microphone.idle_hint')}</p>
    {/if}
  </div>

  <div class="flex items-center justify-center">
    {#if testing}
      <Button variant="outline" onclick={stopTest}>
        <Square class="h-4 w-4 mr-2" />
        {t('settings.general.microphone.stop')}
      </Button>
    {:else}
      <Button onclick={startTest}>
        <Mic class="h-4 w-4 mr-2" />
        {t('settings.general.microphone.test_mic')}
      </Button>
    {/if}
  </div>
</div>
