<script lang="ts">
  import { Settings } from '@lucide/svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import { modelState } from '$lib/stores/modelState';
  import { t } from '$lib/i18n';
  import { navigate } from '$lib/router';
  import KeyChip from '$lib/components/KeyChip.svelte';
  import Logo from '$lib/components/Logo.svelte';
  import { Button } from '$lib/components/ui/button';

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
    $modelState.kind === 'ready' && ($modelState as any).whisper === true
  );
</script>

<div class="h-full flex flex-col p-8 relative">
  <!-- Ambient glow behind hero -->
  <div class="absolute inset-0 pointer-events-none bg-[radial-gradient(circle_at_center,oklch(0.58_0.21_257/0.06),transparent_60%)]"></div>

  <div class="flex-1 flex flex-col items-center justify-center gap-6 relative">
    {#if $settings && !$settings.onboardingComplete}
      <Logo size={80} />
      <div class="text-center max-w-sm">
        <h1 class="text-2xl font-semibold mb-2">{t('home.setup_incomplete_title')}</h1>
        <p class="text-sm text-muted-foreground mb-6">{t('home.setup_incomplete_body')}</p>
        <Button onclick={() => navigate('wizard')}>{t('home.rerun_wizard')}</Button>
      </div>
    {:else if $modelState.kind === 'error' || ($modelState.kind === 'ready' && !($modelState as any).whisper)}
      <div class="w-16 h-16 rounded-full bg-destructive/10 flex items-center justify-center">
        <span class="text-destructive text-2xl">⚠</span>
      </div>
      <div class="text-center max-w-sm">
        <h1 class="text-xl font-semibold mb-2">{t('home.sidecar_down_title')}</h1>
        <p class="text-sm text-muted-foreground mb-6">{t('home.sidecar_down_body')}</p>
        <Button onclick={() => navigate('settings')}>{t('home.retry')}</Button>
      </div>
    {:else if $modelState.kind === 'loading' || $modelState.kind === 'reloading'}
      <Logo size={64} />
      <p class="text-sm text-muted-foreground">{t('home.loading')}</p>
    {:else if $modelState.kind === 'idle' || ($modelState.kind === 'ready' && !canDictate)}
      <Logo size={64} />
      <div class="text-center max-w-sm">
        <h1 class="text-xl font-semibold mb-2">{t('home.models_not_loaded_title')}</h1>
        <p class="text-sm text-muted-foreground mb-6">{t('home.models_not_loaded_body')}</p>
        <Button onclick={() => navigate('settings')}>{t('home.open_settings')}</Button>
      </div>
    {:else if canDictate && $settings}
      <h1 class="text-2xl font-semibold">{t('home.title')}</h1>
      <KeyChip
        label={HOTKEY_LABEL[$settings.hotkey] || ''}
        glyph={HOTKEY_GLYPH[$settings.hotkey]}
        size="lg"
        selected
      />
      <div class="mt-8 px-4 py-2 rounded-full bg-muted/50 backdrop-blur-sm">
        <p class="text-xs text-muted-foreground">
          {#if $modelState.kind === 'ready'}
            🟢 {($modelState as any).whisper ? '✓ STT' : '✗ STT'} · {($modelState as any).llama ? '✓ LLM' : '✗ LLM'}
          {/if}
        </p>
      </div>
    {/if}
  </div>

  <button
    onclick={() => navigate('settings')}
    class="absolute bottom-4 right-4 flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
  >
    <Settings class="h-3.5 w-3.5" />
    Settings
  </button>
</div>
