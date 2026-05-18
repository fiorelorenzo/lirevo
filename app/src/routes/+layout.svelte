<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { ModeWatcher } from 'mode-watcher';
  import { Toaster, toast } from 'svelte-sonner';

  import Titlebar from '$lib/components/Titlebar.svelte';
  import RecordingIndicator from '$lib/components/RecordingIndicator.svelte';

  import { initI18n } from '$lib/i18n';
  import { loadSettings } from '$lib/stores/settings.svelte';
  import { toasts } from '$lib/stores/toasts';

  // Side-effect imports: instantiate stores so they subscribe to backend events.
  import '$lib/stores/modelState';
  import '$lib/stores/recording';
  import '$lib/stores/downloads';

  let { children } = $props();
  let i18nReady = $state(false);

  // Derive page title from current route.
  let titlebarLabel = $derived.by(() => {
    const path = page.url.pathname;
    if (path.startsWith('/settings')) return 'Settings';
    if (path.startsWith('/wizard')) return 'Setup';
    if (path.startsWith('/model-manager')) return 'Model Manager';
    return '';
  });

  // Forward backend toasts (already arriving into the local `toasts` store) to Sonner.
  const shown = new Set<number>();
  toasts.subscribe((arr) => {
    for (const t of arr) {
      if (!shown.has(t.id)) {
        shown.add(t.id);
        if (t.kind === 'info')  toast.info(t.message);
        if (t.kind === 'warn')  toast.warning(t.message);
        if (t.kind === 'error') toast.error(t.message);
      }
    }
  });

  onMount(async () => {
    await initI18n('en');
    i18nReady = true;
    void loadSettings();
  });
</script>

<ModeWatcher defaultMode="system" />
<Toaster richColors position="bottom-right" closeButton />

<main class="flex flex-col h-screen bg-background text-foreground overflow-hidden">
  <Titlebar title={titlebarLabel} />
  <div class="flex-1 overflow-hidden relative">
    {#if i18nReady}
      {@render children()}
    {/if}
    <RecordingIndicator />
  </div>
</main>
