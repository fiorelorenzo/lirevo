<script lang="ts">
  import { fly } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import { Button } from '$lib/components/ui/button';
  import * as AlertDialog from '$lib/components/ui/alert-dialog';
  import StepIndicator from '$lib/components/StepIndicator.svelte';
  import Language from '$lib/components/wizard/Language.svelte';
  import Downloads from '$lib/components/wizard/Downloads.svelte';
  import Permissions from '$lib/components/wizard/Permissions.svelte';
  import General from '$lib/components/wizard/General.svelte';
  import { lda } from '$lib/tauri';
  import { withErrorToast } from '$lib/stores/toasts';
  import { t } from '$lib/i18n';
  import {
    defaultStepState,
    type WizardStepState,
  } from '$lib/components/wizard/step-state';

  let step = $state(0);
  let direction = $state<'forward' | 'backward'>('forward');
  let skipPromptOpen = $state(false);

  const STEPS = 4;

  // One bindable next-state per step. Each step component mutates its slot
  // via `$bindable`; the wizard footer reads the slot matching the active
  // step. Re-mounting a step on back/forward navigation resets its slot
  // via the step's own $effect.
  let stepStates = $state<WizardStepState[]>(
    Array.from({ length: STEPS }, () => defaultStepState()),
  );
  let activeState = $derived(stepStates[step] ?? defaultStepState());

  let isLastStep = $derived(step === STEPS - 1);
  let nextPending = $state(false);

  function next() { direction = 'forward'; step = Math.min(step + 1, STEPS - 1); }
  function back() { direction = 'backward'; step = Math.max(step - 1, 0); }

  async function finish() {
    await withErrorToast(t('wizard.error.complete'), () => lda.completeWizard());
  }

  async function onFooterNext() {
    const handler = activeState.onNextClick;
    if (!handler) {
      // No handler bound (shouldn't happen during normal flow): fall back
      // to plain advance / finish so the user is never stuck.
      isLastStep ? await finish() : next();
      return;
    }
    nextPending = true;
    try {
      const result = await handler();
      if (result && typeof result === 'object' && result.deferAdvance) {
        // Step owns the advance (e.g. Cleanup awaiting download:complete).
        return;
      }
    } finally {
      nextPending = false;
    }
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
        {#if step === 0}<Language onnext={next} bind:nextState={stepStates[0]} />
        {:else if step === 1}<Downloads onnext={next} bind:nextState={stepStates[1]} />
        {:else if step === 2}<Permissions onnext={next} bind:nextState={stepStates[2]} />
        {:else if step === 3}<General onfinish={finish} bind:nextState={stepStates[3]} />
        {/if}
      </div>
    {/key}
  </div>

  <footer class="px-8 py-4 border-t border-border flex items-center justify-between gap-4">
    {#if step > 0}
      <Button variant="ghost" onclick={back}>{t('wizard.common.back')}</Button>
    {:else}
      <span></span>
    {/if}

    <div class="flex items-center gap-4">
      {#if !isLastStep}
        <button
          onclick={() => (skipPromptOpen = true)}
          class="text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          {t('wizard.common.skip')}
        </button>
      {/if}
      <Button
        onclick={onFooterNext}
        disabled={!activeState.canNext || nextPending}
      >
        {activeState.nextLabel ?? (isLastStep ? t('wizard.common.done') : t('wizard.common.next'))}
      </Button>
    </div>
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
