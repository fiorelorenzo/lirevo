<script lang="ts">
  import { Settings, AlertTriangle, Trash2 } from '@lucide/svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { modelState } from '$lib/stores/modelState';
  import { permissionsState } from '$lib/stores/permissions';
  import { dictationHistory } from '$lib/stores/dictationHistory';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';
  import Logo from '$lib/components/Logo.svelte';
  import Spinner from '$lib/components/Spinner.svelte';
  import HistoryEmpty from '$lib/components/home/HistoryEmpty.svelte';
  import HistoryList from '$lib/components/home/HistoryList.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import { lda } from '$lib/tauri';
  import { formatHotkey } from '$lib/hotkey';
  import { withErrorToast } from '$lib/stores/toasts';

  let canDictate = $derived(
    $modelState.kind === 'ready' && ($modelState as any).stt === true
  );

  // When Accessibility is observed granted, try to (re)install the hotkey
  // listener. The listener bypasses the cached AXIsProcessTrusted check
  // and probes CGEventTapCreate directly, so it picks up a fresh grant
  // without needing an app restart.
  //
  // Fires on first observation if already granted (because the backend may
  // have skipped the at-launch install: it now defers when AX is missing,
  // so the first-run grant-via-wizard flow needs the home page to drive
  // the install). Steady-state granted does not fire — only the transition
  // INTO granted (which includes the null → granted "first observation").
  // The backend's reinstall is idempotent so a redundant fire is safe.
  let lastAccessibility: typeof $permissionsState.accessibility = null;
  $effect(() => {
    const current = $permissionsState.accessibility;
    if (lastAccessibility !== 'granted' && current === 'granted') {
      void withErrorToast(t('home.error.retry_hotkey'), () => lda.retryHotkeyInstall());
    }
    lastAccessibility = current;
  });

  let missingAccessibility = $derived($permissionsState.accessibility === 'denied');
  // Treat `not_determined` as missing too: dictation can't capture audio
  // until TCC actually flips to `granted`. The user reaches this state
  // when they skip the Microphone wizard step without pressing Test.
  let missingMicrophone = $derived(
    $permissionsState.microphone === 'denied' ||
    $permissionsState.microphone === 'not_determined',
  );
  let microphoneNeverAsked = $derived($permissionsState.microphone === 'not_determined');
  let hasPermissionIssue = $derived(missingAccessibility || missingMicrophone);

  let onboardingIncomplete = $derived($settings != null && !$settings.onboardingComplete);
  let modelsErrored = $derived(
    $modelState.kind === 'error' || ($modelState.kind === 'ready' && !($modelState as any).stt),
  );
  let modelsLoading = $derived(
    $modelState.kind === 'loading' || $modelState.kind === 'reloading',
  );
  let modelsNotLoaded = $derived(
    $modelState.kind === 'idle' || ($modelState.kind === 'ready' && !canDictate),
  );

  // Render the configured hotkey as a single joined chip string (e.g. "⌥ right"
  // or "⌃ ⇧ K"). macOS is the only shipped platform today.
  let hotkeyGlyph = $derived($settings ? formatHotkey($settings.hotkey, 'macos').join(' ') : undefined);

  async function grantMicrophone() {
    await withErrorToast(t('home.error.grant_microphone'), () => lda.promptMicrophone());
  }
  async function openAccessibilitySettings() {
    await withErrorToast(t('home.error.open_accessibility_settings'), () =>
      lda.openSystemSettingsAccessibility(),
    );
  }
  async function openMicrophoneSettings() {
    await withErrorToast(t('home.error.open_microphone_settings'), () =>
      lda.openSystemSettingsMicrophone(),
    );
  }

  // --- History ----------------------------------------------------------
  let items = $derived($dictationHistory.items);
  let hasMore = $derived($dictationHistory.hasMore);
  let historyLoading = $derived($dictationHistory.loading);

  let selectedId = $state<number | null>(null);
  let clearOpen = $state(false);
  let clearing = $state(false);

  $effect(() => {
    void dictationHistory.load();
  });

  // Infinite scroll: auto-fetch the next history page when a sentinel near the
  // bottom of the scroll area comes into view (pre-loads 200px early).
  let scrollContainer = $state<HTMLElement | null>(null);
  let loadSentinel = $state<HTMLElement | null>(null);
  $effect(() => {
    const root = scrollContainer;
    const sentinel = loadSentinel;
    if (!root || !sentinel || !hasMore) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting) && hasMore && !historyLoading) {
          void dictationHistory.loadMore();
        }
      },
      { root, rootMargin: '200px' },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  });

  function toggleSelect(id: number) {
    selectedId = selectedId === id ? null : id;
  }

  async function deleteOne(id: number) {
    if (selectedId === id) selectedId = null;
    await withErrorToast('Could not delete dictation', () => dictationHistory.removeOne(id));
  }

  async function confirmClear() {
    clearing = true;
    try {
      await withErrorToast('Could not clear history', () => dictationHistory.clearAll());
      selectedId = null;
      clearOpen = false;
    } finally {
      clearing = false;
    }
  }
</script>

<div class="h-full flex flex-col">
  <!--
    Compact status header. Always shows the Settings link; the left side
    reflects readiness (ready hero collapsed to a slim row) or the active
    loading / error / onboarding / permission state in compact form.
  -->
  <header
    data-tauri-drag-region
    class="relative flex items-center justify-between gap-3 border-b border-border/60 px-5 py-3 backdrop-blur-md"
  >
    <div class="flex min-w-0 items-center gap-3 pointer-events-none">
      {#if onboardingIncomplete}
        <Logo size={28} />
        <div class="min-w-0">
          <p class="truncate text-sm font-medium">{t('home.setup_incomplete_title')}</p>
          <p class="truncate text-xs text-muted-foreground">{t('home.setup_incomplete_body')}</p>
        </div>
      {:else if modelsErrored}
        <div class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-destructive/10">
          <AlertTriangle class="h-4 w-4 text-destructive" />
        </div>
        <div class="min-w-0">
          <p class="truncate text-sm font-medium">{t('home.sidecar_down_title')}</p>
          {#if $modelState.kind === 'error' && ($modelState as any).reason}
            <p class="truncate font-mono text-xs text-muted-foreground">
              {($modelState as any).reason}
            </p>
          {/if}
        </div>
      {:else if modelsLoading}
        <Logo size={28} loading />
        <p class="truncate text-sm text-muted-foreground animate-pulse">
          {$modelState.kind === 'reloading'
            ? (($modelState as any).reason ?? t('home.loading'))
            : t('home.loading')}
        </p>
      {:else if modelsNotLoaded}
        <Logo size={28} />
        <p class="truncate text-sm font-medium">{t('home.models_not_loaded_title')}</p>
      {:else if canDictate && $settings}
        <span class="relative inline-flex h-2 w-2 shrink-0">
          <span class="absolute inset-0 rounded-full bg-success/60 animate-ping"></span>
          <span class="relative inline-flex h-full w-full rounded-full bg-success"></span>
        </span>
        <p class="truncate text-sm font-medium">{t('home.title')}</p>
        {#if hotkeyGlyph}
          <span class="rounded-md border border-border bg-surface px-1.5 py-0.5 font-mono text-xs">
            {hotkeyGlyph}
          </span>
        {/if}
      {/if}
    </div>

    <div class="flex shrink-0 items-center gap-1">
      {#if onboardingIncomplete}
        <Button size="sm" onclick={() => navigate('wizard')}>{t('home.rerun_wizard')}</Button>
      {:else if modelsErrored}
        <Button size="sm" onclick={() => navigate('settings')}>{t('home.retry')}</Button>
        <Button size="sm" variant="outline" onclick={() => navigate('wizard')}>
          {t('home.rerun_wizard')}
        </Button>
      {:else if modelsNotLoaded}
        <Button size="sm" onclick={() => navigate('settings')}>{t('home.open_settings')}</Button>
      {/if}

      {#if items.length > 0}
        <button
          onclick={() => (clearOpen = true)}
          class="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
        >
          <Trash2 class="h-3.5 w-3.5" />
          Clear
        </button>
      {/if}
      <button
        onclick={() => navigate('settings')}
        aria-label={t('home.settings')}
        class="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
      >
        <Settings class="h-3.5 w-3.5" />
        {t('home.settings')}
      </button>
    </div>
  </header>

  {#if hasPermissionIssue}
    <div class="relative flex items-start gap-3 border-b border-warning/40 bg-warning/10 px-5 py-3">
      <AlertTriangle class="mt-0.5 h-5 w-5 shrink-0 text-warning" />
      <div class="min-w-0 flex-1">
        <p class="text-sm font-medium">Permissions missing</p>
        <p class="mt-1 text-xs text-muted-foreground">
          {#if missingAccessibility && missingMicrophone}
            macOS Accessibility (needed for the hotkey + text injection) and Microphone are both missing. Grant them below.
          {:else if missingAccessibility}
            macOS Accessibility is blocked. The hotkey won't fire and lda can't type into other apps until it's granted.
          {:else if microphoneNeverAsked}
            macOS Microphone access hasn't been requested yet. Click Grant to bring up the system prompt.
          {:else}
            macOS Microphone access is blocked. Dictation won't capture any audio until it's granted.
          {/if}
        </p>
        <div class="mt-3 flex flex-wrap gap-2">
          {#if missingAccessibility}
            <Button size="sm" variant="outline" onclick={openAccessibilitySettings}>
              Open Accessibility settings
            </Button>
          {/if}
          {#if missingMicrophone}
            {#if microphoneNeverAsked}
              <Button size="sm" variant="outline" onclick={grantMicrophone}>
                Grant microphone access
              </Button>
            {:else}
              <Button size="sm" variant="outline" onclick={openMicrophoneSettings}>
                Open Microphone settings
              </Button>
            {/if}
          {/if}
        </div>
      </div>
    </div>
  {/if}

  <!-- History area -->
  <div bind:this={scrollContainer} class="relative flex flex-1 flex-col overflow-y-auto">
    {#if items.length === 0}
      {#if historyLoading}
        <div class="flex flex-1 items-center justify-center">
          <Spinner size="lg" label="Loading history" />
        </div>
      {:else}
        <HistoryEmpty
          hotkeyChips={canDictate && $settings ? formatHotkey($settings.hotkey, 'macos') : undefined}
          mode={$settings?.activationMode}
        />
      {/if}
    {:else}
      <div class="flex flex-col gap-3 p-5">
        <HistoryList
          {items}
          {selectedId}
          onSelect={toggleSelect}
          onDelete={deleteOne}
        />

        {#if hasMore}
          <div bind:this={loadSentinel} class="flex justify-center py-2">
            {#if historyLoading}
              <Spinner size="sm" label="Loading more" />
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<AlertDialog.Root bind:open={clearOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Clear dictation history?</AlertDialog.Title>
      <AlertDialog.Description>
        This permanently deletes every saved dictation from this device. This can't be undone.
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Action variant="destructive" disabled={clearing} onclick={confirmClear}>
        {#if clearing}
          <Spinner size="sm" label="Clearing" />
        {:else}
          Clear history
        {/if}
      </AlertDialog.Action>
      <AlertDialog.Cancel variant="default" disabled={clearing}>Cancel</AlertDialog.Cancel>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
