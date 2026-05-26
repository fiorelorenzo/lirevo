<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Keyboard, ArrowRight, MousePointerClick, Type } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { withErrorToast } from '$lib/stores/toasts';
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
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  // We poll the TCC status while this step is mounted (the user usually
  // grants the permission in System Settings, not in the app, so we need
  // to pick up the flip without focus events) but DO NOT auto-advance
  // when it flips to `granted`. Every other wizard step requires an
  // explicit Next click; auto-advancing here was a one-off "magic moment"
  // that made the flow feel inconsistent — the user reported being
  // surprised by the jump. Better to show the green "granted" status
  // and let them click Next when they're ready.
  async function refresh() {
    status = await lda.checkAccessibility();
  }

  async function prompt() {
    await withErrorToast(t('wizard.accessibility.error.prompt'), () =>
      lda.promptAccessibility(),
    );
  }

  onMount(() => {
    void refresh();
    pollTimer = setInterval(refresh, 1000);
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  $effect(() => {
    nextState = {
      canNext: true,
      onNextClick: onnext,
    };
  });
</script>

<div class="flex flex-col items-center justify-center min-h-full text-center max-w-md mx-auto gap-6">
  <h1 class="text-2xl font-semibold tracking-tight animate-in fade-in slide-in-from-bottom-2 duration-500">
    {t('wizard.accessibility.title')}
  </h1>
  <p class="text-sm text-muted-foreground animate-in fade-in duration-500 delay-100">
    {t('wizard.accessibility.body')}
  </p>

  <div class="flex items-center gap-3 p-4 bg-muted/30 rounded-lg animate-in fade-in duration-500 delay-200">
    <Keyboard class="h-5 w-5 text-muted-foreground" />
    <ArrowRight class="h-4 w-4 text-muted-foreground" />
    <MousePointerClick class="h-5 w-5 text-muted-foreground" />
    <ArrowRight class="h-4 w-4 text-muted-foreground" />
    <Type class="h-5 w-5 text-muted-foreground" />
  </div>

  <PermissionStatus
    {status}
    granted_label={t('wizard.accessibility.granted')}
    denied_label={t('wizard.accessibility.denied')}
  />

  {#if status !== 'granted'}
    <div class="animate-in fade-in duration-400 delay-300">
      <Button onclick={prompt}>{t('wizard.accessibility.open_settings')}</Button>
    </div>
  {/if}
</div>
