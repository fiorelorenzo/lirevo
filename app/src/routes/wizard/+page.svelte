<script lang="ts">
  import { fly } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import { Button } from '$lib/components/ui/button';
  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import StepIndicator from '$lib/components/StepIndicator.svelte';
  import Welcome from '$lib/components/wizard/Welcome.svelte';
  import Accessibility from '$lib/components/wizard/Accessibility.svelte';
  import Microphone from '$lib/components/wizard/Microphone.svelte';
  import Models from '$lib/components/wizard/Models.svelte';
  import Hotkey from '$lib/components/wizard/Hotkey.svelte';
  import { lda } from '$lib/tauri';
  import { t } from '$lib/i18n';

  let step = $state(0);
  let direction = $state<'forward' | 'backward'>('forward');
  let skipPromptOpen = $state(false);

  const STEPS = 5;

  function next() { direction = 'forward'; step = Math.min(step + 1, STEPS - 1); }
  function back() { direction = 'backward'; step = Math.max(step - 1, 0); }

  async function finish() {
    await lda.completeWizard();
  }
</script>

<div class="h-full flex flex-col">
  <header class="px-8 pt-6">
    <StepIndicator {step} total={STEPS} />
  </header>

  <div class="flex-1 relative overflow-hidden">
    {#key step}
      <div
        in:fly={{ x: direction === 'forward' ? 40 : -40, duration: 400, easing: quintOut }}
        out:fly={{ x: direction === 'forward' ? -40 : 40, duration: 300, easing: quintOut }}
        class="absolute inset-0 px-8 py-6 overflow-y-auto"
      >
        {#if step === 0}<Welcome onnext={next} />
        {:else if step === 1}<Accessibility onnext={next} />
        {:else if step === 2}<Microphone onnext={next} />
        {:else if step === 3}<Models onnext={next} />
        {:else if step === 4}<Hotkey onfinish={finish} />
        {/if}
      </div>
    {/key}
  </div>

  <footer class="px-8 py-4 border-t border-border flex items-center justify-between">
    {#if step > 0}
      <Button variant="ghost" onclick={back}>{t('wizard.common.back')}</Button>
    {:else}
      <span></span>
    {/if}
    <button
      onclick={() => (skipPromptOpen = true)}
      class="text-sm text-muted-foreground hover:text-foreground transition-colors"
    >
      {t('wizard.common.skip')}
    </button>
  </footer>
</div>

<AlertDialog.Root bind:open={skipPromptOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>{t('wizard.common.skip_confirm_title')}</AlertDialog.Title>
      <AlertDialog.Description>{t('wizard.common.skip_confirm_body')}</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Action variant="destructive" onclick={() => { skipPromptOpen = false; void finish(); }}>
        {t('wizard.common.skip_confirm_skip')}
      </AlertDialog.Action>
      <AlertDialog.Cancel variant="default">{t('wizard.common.skip_confirm_cancel')}</AlertDialog.Cancel>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
