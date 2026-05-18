<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Keyboard, ArrowRight, MousePointerClick, Type } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  // Only auto-advance if we observed a transition from non-granted to granted
  // during this view. If the user navigates back to this step with permission
  // already granted, we stay put — they must click Next themselves.
  let observedNonGranted = $state(false);

  async function refresh() {
    const newStatus = await lda.checkAccessibility();
    status = newStatus;
    if (newStatus !== 'granted') {
      observedNonGranted = true;
    } else if (observedNonGranted && pollTimer) {
      // Transitioned denied/not_determined → granted during this view.
      clearInterval(pollTimer);
      pollTimer = null;
      setTimeout(() => onnext(), 800);
    }
  }

  async function prompt() {
    await lda.promptAccessibility();
  }

  onMount(() => {
    void refresh();
    pollTimer = setInterval(refresh, 1000);
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<div class="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto gap-6">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.accessibility.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.accessibility.body')}</p>

  <div class="flex items-center gap-3 p-4 bg-muted/30 rounded-lg">
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

  {#if status === 'granted'}
    <Button onclick={onnext}>{t('wizard.common.next')}</Button>
  {:else}
    <Button onclick={prompt}>{t('wizard.accessibility.open_settings')}</Button>
    <p class="text-xs text-muted-foreground">{t('wizard.accessibility.polling')}</p>
  {/if}
</div>
