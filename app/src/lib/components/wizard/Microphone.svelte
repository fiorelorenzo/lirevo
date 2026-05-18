<script lang="ts">
  import { onMount } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let result = $state<'ok' | 'no_audio' | 'error' | null>(null);
  let errorMessage = $state<string | null>(null);

  // Threshold (RMS 0..1) above which we consider the test "detected audio".
  // Room noise sits around 0.005-0.02; speech ~0.05-0.3.
  const AUDIO_THRESHOLD = 0.04;

  async function refresh() {
    status = await lda.checkMicrophone();
  }

  onMount(refresh);

  async function testMic() {
    testing = true;
    result = null;
    errorMessage = null;
    try {
      const peak = await lda.testMic();
      result = peak >= AUDIO_THRESHOLD ? 'ok' : 'no_audio';
    } catch (e) {
      result = 'error';
      errorMessage = String(e);
    } finally {
      testing = false;
      // Re-check permission — TCC prompt may have fired during the test.
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
    {testing ? t('wizard.microphone.testing') : t('wizard.microphone.test_mic')}
  </Button>

  {#if result === 'ok'}
    <p class="text-sm font-medium text-success">{t('wizard.microphone.tested_ok')}</p>
  {:else if result === 'no_audio'}
    <div class="text-sm space-y-1">
      <p class="font-medium text-warning">{t('wizard.microphone.tested_no_audio')}</p>
      <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_audio_hint')}</p>
    </div>
  {:else if result === 'error'}
    <div class="text-sm space-y-1">
      <p class="font-medium text-destructive">{t('wizard.microphone.tested_error')}</p>
      {#if errorMessage}<p class="text-xs text-muted-foreground font-mono">{errorMessage}</p>{/if}
    </div>
  {/if}

  <Button disabled={status !== 'granted'} onclick={onnext}>
    {t('wizard.common.next')}
  </Button>
</div>
