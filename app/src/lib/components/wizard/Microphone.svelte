<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Select from '$lib/components/ui/select';
  import { Label } from '$lib/components/ui/label';
  import { Mic, Square } from '@lucide/svelte';
  import PermissionStatus from '$lib/components/PermissionStatus.svelte';
  import { lda, type PermissionStatus as Status, type InputDeviceEntry } from '$lib/tauri';
  import { settings, updateSettings } from '$lib/stores/settings.svelte';
  import { t } from '$lib/i18n';

  interface Props { onnext: () => void; }
  let { onnext }: Props = $props();

  let status = $state<Status | null>(null);
  let testing = $state(false);
  let devices = $state<InputDeviceEntry[]>([]);
  let selectedDevice = $state<string | null>(null);

  type Result =
    | { kind: 'ok'; peak: number; device: string }
    | { kind: 'no_capture' }
    | { kind: 'cancelled' }
    | { kind: 'no_audio'; peak: number; device: string }
    | { kind: 'error'; message: string };

  let result = $state<Result | null>(null);

  async function refreshPermission() {
    status = await lda.checkMicrophone();
  }

  async function refreshDevices() {
    try {
      devices = await lda.listInputDevices();
      // If we don't have a stored device, pre-select the system default.
      if (selectedDevice === null && $settings) {
        selectedDevice = $settings.inputDeviceName ?? null;
      }
    } catch (e) {
      console.warn('listInputDevices failed', e);
    }
  }

  onMount(async () => {
    await Promise.all([refreshPermission(), refreshDevices()]);
    if ($settings) selectedDevice = $settings.inputDeviceName ?? null;
  });

  onDestroy(() => {
    if (testing) void lda.cancelTestMic();
  });

  async function startTest() {
    testing = true;
    result = null;
    try {
      const res = await lda.testMic(selectedDevice);
      if (res.cancelled) {
        result = { kind: 'cancelled' };
      } else if (res.sampleCount === 0) {
        result = { kind: 'no_capture' };
      } else if (res.detected) {
        result = { kind: 'ok', peak: res.peak, device: res.deviceLabel };
      } else {
        result = { kind: 'no_audio', peak: res.peak, device: res.deviceLabel };
      }
    } catch (e) {
      result = { kind: 'error', message: String(e) };
    } finally {
      testing = false;
      await refreshPermission();
    }
  }

  async function stopTest() {
    await lda.cancelTestMic();
    // testMic() resolves with cancelled=true; finally block runs.
  }

  async function selectDevice(name: string | null) {
    selectedDevice = name;
    await updateSettings({ inputDeviceName: name });
    // Clear any stale result so the user re-tests with the new device.
    result = null;
  }
</script>

<div class="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto gap-5">
  <h1 class="text-2xl font-semibold tracking-tight">{t('wizard.microphone.title')}</h1>
  <p class="text-sm text-muted-foreground">{t('wizard.microphone.body')}</p>

  <PermissionStatus
    {status}
    granted_label={t('wizard.microphone.granted')}
    denied_label={t('wizard.microphone.denied')}
  />

  <div class="w-full max-w-xs space-y-2 text-left">
    <Label class="text-xs uppercase tracking-wide text-muted-foreground">
      {t('wizard.microphone.input_device')}
    </Label>
    <Select.Root
      type="single"
      value={selectedDevice ?? '__default__'}
      onValueChange={(v) => selectDevice(v === '__default__' ? null : (v ?? null))}
      disabled={testing || devices.length === 0}
    >
      <Select.Trigger class="w-full">
        {selectedDevice
          ?? (devices.find((d) => d.isDefault)?.name
              ? `${devices.find((d) => d.isDefault)?.name} (${t('wizard.microphone.default')})`
              : t('wizard.microphone.default'))}
      </Select.Trigger>
      <Select.Content>
        <Select.Item value="__default__">
          {devices.find((d) => d.isDefault)?.name
            ? `${devices.find((d) => d.isDefault)?.name} (${t('wizard.microphone.default')})`
            : t('wizard.microphone.default')}
        </Select.Item>
        {#each devices as d (d.name)}
          <Select.Item value={d.name}>
            {d.name}{d.isDefault ? ` (${t('wizard.microphone.default')})` : ''}
          </Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  <div class="flex items-center gap-2">
    {#if testing}
      <Button variant="outline" onclick={stopTest}>
        <Square class="h-4 w-4 mr-2" />
        {t('wizard.microphone.stop')}
      </Button>
    {:else}
      <Button onclick={startTest} disabled={status === 'denied'}>
        <Mic class="h-4 w-4 mr-2" />
        {t('wizard.microphone.test_mic')}
      </Button>
    {/if}
  </div>

  {#if testing}
    <p class="text-sm text-muted-foreground">
      {t('wizard.microphone.listening')}
    </p>
  {:else if result}
    {#if result.kind === 'ok'}
      <div class="text-sm space-y-1">
        <p class="font-medium text-success">
          ✓ {t('wizard.microphone.tested_ok')}
          <span class="text-xs text-muted-foreground tabular-nums">
            (peak {(result.peak * 100).toFixed(0)}%)
          </span>
        </p>
        <p class="text-xs text-muted-foreground font-mono">{result.device}</p>
      </div>
    {:else if result.kind === 'no_audio'}
      <div class="text-sm space-y-1">
        <p class="font-medium text-warning">{t('wizard.microphone.tested_no_audio')}</p>
        <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_audio_hint')}</p>
      </div>
    {:else if result.kind === 'no_capture'}
      <div class="text-sm space-y-1">
        <p class="font-medium text-destructive">{t('wizard.microphone.tested_no_capture')}</p>
        <p class="text-xs text-muted-foreground">{t('wizard.microphone.tested_no_capture_hint')}</p>
      </div>
    {:else if result.kind === 'cancelled'}
      <p class="text-sm text-muted-foreground">{t('wizard.microphone.tested_cancelled')}</p>
    {:else if result.kind === 'error'}
      <div class="text-sm space-y-1">
        <p class="font-medium text-destructive">{t('wizard.microphone.tested_error')}</p>
        <p class="text-xs text-muted-foreground font-mono">{result.message}</p>
      </div>
    {/if}
  {/if}

  <Button disabled={status !== 'granted'} onclick={onnext}>
    {t('wizard.common.next')}
  </Button>
</div>
