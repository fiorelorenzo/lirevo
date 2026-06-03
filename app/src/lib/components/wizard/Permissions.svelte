<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic, Keyboard, Check, Loader2 } from '@lucide/svelte';
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
    // Poll while mounted: grants happen out of process (System Settings or the
    // OS prompt), so we pick up the flip without focus events.
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

{#snippet row(
  Icon: typeof Mic,
  label: string,
  helper: string,
  status: Status | null,
  grant: () => void,
  openSettings: () => void,
  pending: boolean,
)}
  <div class="p-4 flex items-center justify-between gap-4">
    <div class="flex items-center gap-3 min-w-0">
      <div
        class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0 transition-colors duration-300
          {status === 'granted' ? 'bg-success/10 text-success' : 'bg-muted text-muted-foreground'}"
      >
        <Icon class="h-5 w-5" />
      </div>
      <div class="min-w-0">
        <div class="font-medium leading-tight">{label}</div>
        <p class="text-xs text-muted-foreground mt-0.5 leading-snug">{helper}</p>
      </div>
    </div>

    <div class="shrink-0">
      {#if status === null}
        <Loader2 class="h-4 w-4 animate-spin text-muted-foreground" />
      {:else if status === 'granted'}
        <span
          class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-success/10 text-success text-xs font-medium whitespace-nowrap"
        >
          <Check class="h-3.5 w-3.5" />
          {t('wizard.permissions.granted')}
        </span>
      {:else if status === 'denied'}
        <Button variant="outline" size="sm" onclick={openSettings}>
          {t('wizard.permissions.open_settings')}
        </Button>
      {:else}
        <Button size="sm" onclick={grant} disabled={pending}>
          {t('wizard.permissions.grant')}
        </Button>
      {/if}
    </div>
  </div>
{/snippet}

<div class="max-w-md mx-auto flex flex-col gap-6">
  <div class="text-center space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.permissions.title')}</h1>
    <p class="text-sm text-muted-foreground">{t('wizard.permissions.body')}</p>
  </div>

  <div
    class="w-full rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden animate-in fade-in duration-500 delay-200"
  >
    {@render row(
      Mic,
      t('wizard.permissions.microphone_label'),
      t('wizard.permissions.microphone_helper'),
      micStatus,
      grantMic,
      openMicSettings,
      micPrompting,
    )}
    {@render row(
      Keyboard,
      t('wizard.permissions.accessibility_label'),
      t('wizard.permissions.accessibility_helper'),
      axStatus,
      grantAccessibility,
      openAxSettings,
      false,
    )}
  </div>
</div>
