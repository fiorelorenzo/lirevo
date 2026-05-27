<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { ModeWatcher } from 'mode-watcher';
  import { Toaster } from 'svelte-sonner';

  import Titlebar from '$lib/components/Titlebar.svelte';

  import { initI18n } from '$lib/i18n';
  import { loadSettings, settings } from '$lib/stores/settings.svelte';
  import { lda } from '$lib/tauri';
  import type { UnlistenFn } from '@tauri-apps/api/event';

  // Side-effect imports: instantiate stores so they subscribe to backend events.
  import '$lib/stores/modelState';
  import '$lib/stores/recording';
  import '$lib/stores/downloads';
  import '$lib/stores/toasts';

  let { children } = $props();
  let i18nReady = $state(false);

  // The transparent recording overlay window loads /overlay and needs
  // none of the regular chrome: no titlebar, no opaque background.
  let isOverlay = $derived(page.url.pathname.startsWith('/overlay'));

  // Derive page title from current route.
  let titlebarLabel = $derived.by(() => {
    const path = page.url.pathname;
    if (path.startsWith('/settings')) return 'Settings';
    if (path.startsWith('/wizard')) return 'Setup';
    return '';
  });

  let unlistenSettings: UnlistenFn | null = null;

  onMount(async () => {
    await initI18n('en');
    i18nReady = true;
    void loadSettings();
    // Backend may rewrite settings out-of-band (e.g. the loader auto-
    // clears llm_model_path when the chosen LLM fails to load). Mirror
    // those changes into the frontend store so reactive UI (footer pill,
    // Settings tab) stays in sync without a manual reload.
    unlistenSettings = await lda.onSettingsChanged((s) => settings.set(s));
  });

  $effect(() => {
    return () => {
      unlistenSettings?.();
    };
  });
</script>

<ModeWatcher defaultMode="system" />
<!--
  Per-toast `closeButton` / `duration` are decided by `stores/toasts.ts`
  based on `ToastKind`: info + success auto-dismiss with no X (they're
  acknowledgements that don't deserve interruption), error stays until
  the user dismisses with the X (so a failure can't disappear before
  it's read). Don't add the global `closeButton` prop here — it would
  force every toast to show an X regardless of intent.
-->
<Toaster richColors position="top-center" />

{#if isOverlay}
  <main class="h-screen overflow-hidden bg-transparent">
    {#if i18nReady}
      {@render children()}
    {/if}
  </main>
{:else}
  <main class="flex flex-col h-screen bg-background text-foreground overflow-hidden">
    <Titlebar title={titlebarLabel} />
    <div class="flex-1 overflow-hidden relative">
      {#if i18nReady}
        {@render children()}
      {/if}
    </div>
  </main>
{/if}
