<script lang="ts">
  import { Settings, AlertTriangle } from '@lucide/svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { modelState } from '$lib/stores/modelState';
  import { permissionsState } from '$lib/stores/permissions';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';
  import KeyChip from '$lib/components/KeyChip.svelte';
  import Logo from '$lib/components/Logo.svelte';
  import { Button } from '$lib/components/ui/button';
  import { lda } from '$lib/tauri';
  import { withErrorToast } from '$lib/stores/toasts';

  const HOTKEY_GLYPH: Record<string, string> = {
    'right-option': '⌥',
    'left-option': '⌥',
    'right-command': '⌘',
    'fn': 'fn',
    'f5': 'F5',
  };
  const HOTKEY_LABEL: Record<string, string> = {
    'right-option': 'right',
    'left-option': 'left',
    'right-command': 'right',
    'fn': 'Globe',
    'f5': '',
  };

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
</script>

<div class="h-full flex flex-col p-8 relative">
  <!-- Ambient glow behind hero -->
  <div class="absolute inset-0 pointer-events-none bg-[radial-gradient(circle_at_center,oklch(0.58_0.21_257/0.06),transparent_60%)]"></div>

  {#if hasPermissionIssue}
    <div class="relative rounded-xl border border-warning/40 bg-warning/10 p-4 mb-4 flex items-start gap-3">
      <AlertTriangle class="h-5 w-5 text-warning shrink-0 mt-0.5" />
      <div class="flex-1 min-w-0">
        <p class="text-sm font-medium">Permissions missing</p>
        <p class="text-xs text-muted-foreground mt-1">
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
        <div class="flex flex-wrap gap-2 mt-3">
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

  <div class="flex-1 flex flex-col items-center justify-center gap-6 relative">
    {#if $settings && !$settings.onboardingComplete}
      <Logo size={80} />
      <div class="text-center max-w-sm">
        <div data-tauri-drag-region class="pointer-events-none">
          <h1 class="text-2xl font-semibold mb-2">{t('home.setup_incomplete_title')}</h1>
          <p class="text-sm text-muted-foreground mb-6">{t('home.setup_incomplete_body')}</p>
        </div>
        <Button onclick={() => navigate('wizard')}>{t('home.rerun_wizard')}</Button>
      </div>
    {:else if $modelState.kind === 'error' || ($modelState.kind === 'ready' && !($modelState as any).stt)}
      <div class="w-16 h-16 rounded-full bg-destructive/10 flex items-center justify-center">
        <AlertTriangle class="h-7 w-7 text-destructive" />
      </div>
      <div class="text-center max-w-sm">
        <h1 class="text-xl font-semibold mb-2">{t('home.sidecar_down_title')}</h1>
        {#if $modelState.kind === 'error' && ($modelState as any).reason}
          <p class="text-xs text-muted-foreground mb-6 font-mono break-words">
            {($modelState as any).reason}
          </p>
        {:else}
          <p class="text-sm text-muted-foreground mb-6">{t('home.sidecar_down_body')}</p>
        {/if}
        <div class="flex items-center justify-center gap-2">
          <Button onclick={() => navigate('settings')}>{t('home.retry')}</Button>
          <Button variant="outline" onclick={() => navigate('wizard')}>{t('home.rerun_wizard')}</Button>
        </div>
      </div>
    {:else if $modelState.kind === 'loading' || $modelState.kind === 'reloading'}
      <Logo size={72} loading />
      <p class="text-sm text-muted-foreground animate-pulse">
        {$modelState.kind === 'reloading'
          ? (($modelState as any).reason ?? t('home.loading'))
          : t('home.loading')}
      </p>
    {:else if $modelState.kind === 'idle' || ($modelState.kind === 'ready' && !canDictate)}
      <Logo size={64} />
      <div class="text-center max-w-sm">
        <h1 class="text-xl font-semibold mb-2">{t('home.models_not_loaded_title')}</h1>
        <p class="text-sm text-muted-foreground mb-6">{t('home.models_not_loaded_body')}</p>
        <Button onclick={() => navigate('settings')}>{t('home.open_settings')}</Button>
      </div>
    {:else if canDictate && $settings}
      <div data-tauri-drag-region class="text-center space-y-2 pointer-events-none">
        <h1 class="text-3xl font-semibold tracking-tight">{t('home.title')}</h1>
        <p class="text-sm text-muted-foreground">{t('home.ready_hint')}</p>
      </div>
      <div class="relative">
        <!-- Soft halo behind the key chip — subtle elevation cue without a hard shadow. -->
        <div class="absolute -inset-8 rounded-[32px] bg-primary/[0.04] blur-2xl pointer-events-none"></div>
        <KeyChip
          label={HOTKEY_LABEL[$settings.hotkey] || ''}
          glyph={HOTKEY_GLYPH[$settings.hotkey]}
          size="lg"
          selected
        />
      </div>
    {/if}
  </div>

  <!--
    Status / settings bar. Hairline-divided footer modeled on Linear/Raycast.
    Shows the model state with two small dots on the left + Settings link on
    the right. Only rendered with content when the canDictate hero is up;
    other branches (loading, error, idle, onboarding) hide the model status
    and just keep the Settings link.
  -->
  <footer class="relative -mx-8 -mb-8 mt-4 px-6 py-3 border-t border-border/60 backdrop-blur-md flex items-center justify-between text-xs">
    <div class="flex items-center gap-4">
      {#if $modelState.kind === 'ready' && canDictate}
        {@const sttReady = ($modelState as any).stt === true}
        {@const llmReady = ($modelState as any).llama === true}
        {@const cleanupConfigured = $settings?.llmModelPath != null}
        <span class="flex items-center gap-1.5">
          <span class="relative inline-flex h-1.5 w-1.5">
            {#if sttReady}
              <span class="absolute inset-0 rounded-full bg-success/60 animate-ping"></span>
              <span class="relative inline-flex h-full w-full rounded-full bg-success"></span>
            {:else}
              <span class="inline-flex h-full w-full rounded-full bg-muted-foreground/30"></span>
            {/if}
          </span>
          <span class={sttReady ? 'text-foreground' : 'text-muted-foreground'}>
            {t('home.status_speech')}
          </span>
        </span>
        {#if cleanupConfigured}
          <span class="text-border" aria-hidden="true">·</span>
          <span
            class="flex items-center gap-1.5"
            title={llmReady ? '' : t('home.status_cleanup_loading')}
          >
            <span class="relative inline-flex h-1.5 w-1.5">
              {#if llmReady}
                <span class="absolute inset-0 rounded-full bg-success/60 animate-ping"></span>
                <span class="relative inline-flex h-full w-full rounded-full bg-success"></span>
              {:else}
                <span class="inline-flex h-full w-full rounded-full bg-muted-foreground/30"></span>
              {/if}
            </span>
            <span class={llmReady ? 'text-foreground' : 'text-muted-foreground'}>
              {llmReady ? t('home.status_cleanup') : t('home.status_cleanup_loading')}
            </span>
          </span>
        {/if}
      {/if}
    </div>
    <button
      onclick={() => navigate('settings')}
      class="flex items-center gap-1.5 text-muted-foreground hover:text-foreground transition-colors px-2 py-1 -my-1 rounded-md hover:bg-muted/60"
    >
      <Settings class="h-3.5 w-3.5" />
      {t('home.settings')}
    </button>
  </footer>
</div>
