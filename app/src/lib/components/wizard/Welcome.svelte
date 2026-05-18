<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import Logo from '$lib/components/Logo.svelte';
  import { Zap, Lock, Keyboard } from '@lucide/svelte';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();
</script>

<div class="min-h-full flex flex-col items-center justify-center text-center gap-8 max-w-md mx-auto">
  <Logo size={96} class="animate-in fade-in zoom-in duration-700" />

  <div class="space-y-3 animate-in fade-in slide-in-from-bottom-2 duration-500 delay-200">
    <h1 class="text-3xl font-semibold tracking-tight">{t('wizard.welcome.title')}</h1>
    <p class="text-base text-muted-foreground leading-relaxed">{t('wizard.welcome.subtitle')}</p>
  </div>

  <div class="flex flex-wrap items-center justify-center gap-2 animate-in fade-in duration-500 delay-300">
    <div class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-muted text-xs font-medium">
      <Zap class="h-3 w-3 text-primary" />
      {t('wizard.welcome.feature_fast')}
    </div>
    <div class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-muted text-xs font-medium">
      <Lock class="h-3 w-3 text-primary" />
      {t('wizard.welcome.feature_private')}
    </div>
    <div class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-muted text-xs font-medium">
      <Keyboard class="h-3 w-3 text-primary" />
      {t('wizard.welcome.feature_hotkey')}
    </div>
  </div>

  <div class="animate-in fade-in zoom-in duration-500 delay-500">
    <Button size="lg" onclick={onnext}>{t('wizard.welcome.next')}</Button>
  </div>
</div>
