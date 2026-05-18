<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let countdown = $state(0); // visible seconds remaining during the test
  let countdownTimer: ReturnType<typeof setInterval> | null = null;

  // Lowered threshold: room noise typically 0.005-0.02, whispered ~0.04,
  // normal speech ~0.1-0.3. 0.02 catches even quiet speech.
  const AUDIO_THRESHOLD = 0.02;

  type Result = 'ok' | 'no_audio' | 'no_capture' | 'error';
  let result = $state<Result | null>(null);
  let detectedDevice = $state<string | null>(null);
  let detectedPeak = $state<number | null>(null);
  let errorMessage = $state<string | null>(null);

  async function refresh() {
    status = await lda.checkMicrophone();
  }

  onMount(refresh);

  onDestroy(() => {
    if (countdownTimer) clearInterval(countdownTimer);
  });

  async function testMic() {
    testing = true;
    countdown = 2;
    result = null;
    errorMessage = null;
    detectedPeak = null;

    // Local visual countdown (separate from backend timing).
    countdownTimer = setInterval(() => {
      countdown = Math.max(0, countdown - 1);
    }, 1000);

    try {
      const res = await lda.testMic();
      detectedDevice = res.deviceLabel;
      detectedPeak = res.peak;
      if (res.sampleCount === 0) {
        result = 'no_capture';
      } else if (res.peak >= AUDIO_THRESHOLD) {
        result = 'ok';
      } else {
        result = 'no_audio';
      }
    } catch (e) {
      result = 'error';
      errorMessage = String(e);
    } finally {
      if (countdownTimer) {
        clearInterval(countdownTimer);
        countdownTimer = null;
      }
      countdown = 0;
      testing = false;
      // Re-check permission — the TCC prompt may have fired during the test.
      await refresh();
    }
  }
</script>

<div class="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto gap-6">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.microphone.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.microphone.body')}</p>

  <PermissionStatus
    {status}
    granted_label={t('wizard.microphone.granted')}
    denied_label={t('wizard.microphone.denied')}
  />

  <Button onclick={testMic} disabled={testing}>
    <Mic class="h-4 w-4 mr-2" />
    {testing
      ? `${t('wizard.microphone.testing')} (${countdown}s)`
      : t('wizard.microphone.test_mic')}
  </Button>

  {#if detectedDevice}
    <p class="text-xs text-muted-foreground">
      {t('wizard.microphone.using')}: <span class="font-mono">{detectedDevice}</span>
    </p>
  {/if}

  {#if result === 'ok'}
    <p class="text-sm font-medium text-success">
      ✓ {t('wizard.microphone.tested_ok')}
      {#if detectedPeak !== null}
        <span class="text-xs text-muted-foreground tabular-nums">
          (peak {(detectedPeak * 100).toFixed(0)}%)
        </span>
      {/if}
    </p>
  {:else if result === 'no_audio'}
    <div class="text-sm space-y-1">
      <p class="font-medium text-warning">{t('wizard.microphone.tested_no_audio')}</p>
      <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_audio_hint')}</p>
      {#if detectedPeak !== null}
        <p class="text-xs text-muted-foreground tabular-nums">
          peak {(detectedPeak * 100).toFixed(1)}% (need ≥ {(AUDIO_THRESHOLD * 100).toFixed(0)}%)
        </p>
      {/if}
    </div>
  {:else if result === 'no_capture'}
    <div class="text-sm space-y-1">
      <p class="font-medium text-destructive">{t('wizard.microphone.tested_no_capture')}</p>
      <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_capture_hint')}</p>
    </div>
  {:else if result === 'error'}
    <div class="text-sm space-y-1">
      <p class="font-medium text-destructive">{t('wizard.microphone.tested_error')}</p>
      {#if errorMessage}
        <p class="text-xs text-muted-foreground font-mono">{errorMessage}</p>
      {/if}
    </div>
  {/if}

  <Button disabled={status !== 'granted'} onclick={onnext}>
    {t('wizard.common.next')}
  </Button>
</div>
