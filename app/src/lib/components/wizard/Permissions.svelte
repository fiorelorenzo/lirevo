<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic, Accessibility as AccessibilityIcon } from '@lucide/svelte';
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

  let micStatus = $state<Status | null>(null);
  let axStatus = $state<Status | null>(null);
  let micPrompting = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  async function refresh() {
    [micStatus, axStatus] = await Promise.all([
      lda.checkMicrophone(),
      lda.checkAccessibility(),
    ]);
  }

  async function grantMic() {
    if (micPrompting) return;
    micPrompting = true;
    try {
      micStatus = await lda.promptMicrophone();
    } catch (e) {
      const reason = e instanceof Error ? e.message : String(e);
      toastError(`${t('wizard.microphone.error.prompt')}: ${reason}`);
    } finally {
      micPrompting = false;
    }
  }

  async function openMicSettings() {
    await withErrorToast(t('wizard.microphone.error.open_settings'), () =>
      lda.openSystemSettingsMicrophone(),
    );
  }

  async function grantAccessibility() {
    await withErrorToast(t('wizard.accessibility.error.prompt'), () =>
      lda.promptAccessibility(),
    );
  }

  async function openAxSettings() {
    await withErrorToast(t('wizard.accessibility.error.prompt'), () =>
      lda.openSystemSettingsAccessibility(),
    );
  }

  onMount(() => {
    void refresh();
    // Poll while mounted: grants happen out of process (System Settings or
    // the OS prompt), so we pick up the flip without focus events.
    pollTimer = setInterval(refresh, 1000);
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  $effect(() => {
    nextState = {
      canNext: micStatus === 'granted' && axStatus === 'granted',
      onNextClick: onnext,
    };
  });
</script>

<div class="max-w-md mx-auto flex flex-col gap-6">
  <div class="text-center space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.permissions.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.permissions.body')}</p>
  </div>

  <div class="w-full rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden animate-in fade-in duration-500 delay-200">
    <!-- Microphone -->
    <div class="p-4 flex flex-col gap-3">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-3 min-w-0">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-muted text-muted-foreground shrink-0">
            <Mic class="h-4 w-4" />
          </div>
          <div class="min-w-0">
            <div class="font-medium">{t('wizard.permissions.microphone_label')}</div>
            <p class="text-xs text-muted-foreground">{t('wizard.permissions.microphone_helper')}</p>
          </div>
        </div>
        <PermissionStatus
          status={micStatus}
          granted_label={t('wizard.permissions.granted')}
          denied_label={t('wizard.permissions.denied')}
          not_determined_label={t('wizard.permissions.denied')}
        />
      </div>

      {#if micStatus !== 'granted' && micStatus !== null}
        <div class="flex items-center gap-2">
          {#if micStatus === 'denied'}
            <Button variant="outline" size="sm" onclick={openMicSettings}>
              {t('wizard.permissions.open_settings')}
            </Button>
          {:else}
            <Button size="sm" onclick={grantMic} disabled={micPrompting}>
              <Mic class="h-4 w-4 mr-1.5" />
              {t('wizard.permissions.grant')}
            </Button>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Accessibility -->
    <div class="p-4 flex flex-col gap-3">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-3 min-w-0">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-muted text-muted-foreground shrink-0">
            <AccessibilityIcon class="h-4 w-4" />
          </div>
          <div class="min-w-0">
            <div class="font-medium">{t('wizard.permissions.accessibility_label')}</div>
            <p class="text-xs text-muted-foreground">{t('wizard.permissions.accessibility_helper')}</p>
          </div>
        </div>
        <PermissionStatus
          status={axStatus}
          granted_label={t('wizard.permissions.granted')}
          denied_label={t('wizard.permissions.denied')}
          not_determined_label={t('wizard.permissions.denied')}
        />
      </div>

      {#if axStatus !== 'granted' && axStatus !== null}
        <div class="flex items-center gap-2">
          <Button size="sm" onclick={grantAccessibility}>
            <AccessibilityIcon class="h-4 w-4 mr-1.5" />
            {t('wizard.permissions.grant')}
          </Button>
          <Button variant="outline" size="sm" onclick={openAxSettings}>
            {t('wizard.permissions.open_settings')}
          </Button>
        </div>
      {/if}
    </div>
  </div>
</div>
