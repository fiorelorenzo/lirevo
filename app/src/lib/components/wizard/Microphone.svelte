<script lang="ts">
  import { onMount } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import { Mic } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status } from '$lib/tauri';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let tested = $state(false);

  async function refresh() {
    status = await lda.checkMicrophone();
  }

  onMount(refresh);

  // For M3 the "test mic" is a UI placebo that marks `tested=true`. The actual
  // TCC prompt fires when the user first records via the real hotkey flow.
  async function testMic() {
    testing = true;
    await new Promise((r) => setTimeout(r, 2000));
    testing = false;
    tested = true;
  }
</script>

<div class="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto gap-6">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.microphone.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.microphone.body')}</p>

  <PermissionStatus
    {status}
    granted_label={t('wizard.microphone.granted')}
    denied_label={t('wizard.microphone.denied')}
  />

  <Button onclick={testMic} disabled={testing}>
    <Mic class="h-4 w-4 mr-2" />
    {testing ? t('wizard.microphone.testing') : (tested ? '✓ Tested' : t('wizard.microphone.test_mic'))}
  </Button>

  <Button disabled={!tested} onclick={onnext}>
    {t('wizard.common.next')}
  </Button>
</div>
