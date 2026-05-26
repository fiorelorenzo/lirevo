<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { toastError, withErrorToast } from '$lib/stores/toasts';
  import { t } from '$lib/i18n';
  import { defaultStepState, type WizardStepState } from './step-state';

  interface Props {
    onnext: () => void;
    nextState?: WizardStepState;
  }
  let {
    onnext,
    nextState = $bindable(defaultStepState()),
  }: Props = $props();

  let status = $state<Status | null>(null);
  let prompting = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  async function refreshPermission() {
    status = await lda.checkMicrophone();
  }

  async function requestPermission() {
    if (prompting) return;
    prompting = true;
    try {
      const next = await lda.promptMicrophone();
      status = next;
      // If macOS auto-denied (or the user clicked Deny), surface the
      // recovery path: the "Open System Settings" CTA below the status
      // becomes the user's way out. The toast only fires on a thrown
      // error from the bridge itself.
    } catch (e) {
      const reason = e instanceof Error ? e.message : String(e);
      toastError(`${t('wizard.microphone.error.prompt')}: ${reason}`);
    } finally {
      prompting = false;
    }
  }

  async function openMicrophoneSettings() {
    await withErrorToast(t('wizard.microphone.error.open_settings'), () =>
      lda.openSystemSettingsMicrophone(),
    );
  }

  onMount(() => {
    void refreshPermission();
    // Poll the TCC status while mounted: the user may grant permission
    // from System Settings (out of process), so we pick up the flip
    // without focus events. The footer Next is gated on `granted`.
    pollTimer = setInterval(refreshPermission, 1000);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  $effect(() => {
    nextState = {
      canNext: status === 'granted',
      onNextClick: onnext,
    };
  });
</script>

<div class="flex flex-col items-center justify-center min-h-full text-center max-w-md mx-auto gap-6">
  <h1 class="text-2xl font-semibold tracking-tight animate-in fade-in slide-in-from-bottom-2 duration-500">
    {t('wizard.microphone.title')}
  </h1>
  <p class="text-sm text-muted-foreground animate-in fade-in duration-500 delay-100">
    {t('wizard.microphone.body')}
  </p>

  <PermissionStatus
    {status}
    granted_label={t('wizard.microphone.granted')}
    denied_label={t('wizard.microphone.denied')}
    not_determined_label={t('wizard.microphone.not_determined')}
  />

  {#if status === 'not_determined'}
    <div class="animate-in fade-in duration-400 delay-300">
      <Button onclick={requestPermission} disabled={prompting}>
        <Mic class="h-4 w-4 mr-2" />
        {t('wizard.microphone.grant')}
      </Button>
    </div>
  {:else if status === 'denied'}
    <div class="animate-in fade-in duration-400 delay-300 space-y-2">
      <p class="text-xs text-muted-foreground max-w-xs">
        {t('wizard.microphone.denied_hint')}
      </p>
      <Button variant="outline" onclick={openMicrophoneSettings}>
        {t('wizard.microphone.open_system_settings')}
      </Button>
    </div>
  {/if}
</div>
