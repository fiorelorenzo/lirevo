<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import { Switch } from "$lib/components/ui/switch";
  import { Slider } from "$lib/components/ui/slider";
  import HotkeyRecorder from "$lib/components/HotkeyRecorder.svelte";
  import MicTest from "$lib/components/MicTest.svelte";
  import ModelStatusPanel from "$lib/components/settings/ModelStatusPanel.svelte";
  import { settings, updateSettings } from "$lib/stores/settings.svelte";
  import { profile, setProfileMode } from "$lib/stores/profile";
  import { backend, type BackendState } from "$lib/stores/backend.svelte";
  import type { ProfileName } from "$lib/tauri";
  import { t } from "$lib/i18n";
  import { navigate } from "$lib/router";
  import { lda, type InputDeviceEntry } from "$lib/tauri";
  import { STT_MODELS, CLEANUP_MODEL } from "$lib/models/catalog";
  import { Zap, Cpu } from "@lucide/svelte";
  import { toastError } from "$lib/stores/toasts";
  import { open } from "@tauri-apps/plugin-shell";
  import { page } from "$app/state";
  import { onMount } from "svelte";

  type Tab = "general" | "models" | "hotkey" | "about";
  const TABS: Tab[] = ["general", "models", "hotkey", "about"];

  function isTab(v: string | null): v is Tab {
    return v === "general" || v === "models" || v === "hotkey" || v === "about";
  }

  const initialTab = page.url.searchParams.get("tab");
  let activeTab: Tab = $state(isTab(initialTab) ? initialTab : "general");

  const LANGUAGE_OPTIONS = [
    { value: "auto", label: t("settings.general.language_auto") },
    { value: "en", label: "English" },
    { value: "it", label: "Italiano" },
    { value: "fr", label: "Français" },
    { value: "de", label: "Deutsch" },
    { value: "es", label: "Español" },
  ];

  const ENERGY_OPTIONS = [
    { value: "auto", label: t("settings.general.energy_auto") },
    { value: "power_saver", label: t("settings.general.energy_power_saver") },
    { value: "balanced", label: t("settings.general.energy_balanced") },
    { value: "performance", label: t("settings.general.energy_performance") },
  ];

  // The live store's `mode` wins once it resolves; before that fall back to
  // the persisted choice so the control shows the right value on first paint.
  let energyMode = $derived($profile?.mode ?? $settings?.profileMode ?? "auto");
  const PROFILE_LABELS: Record<ProfileName, string> = {
    powerSaver: "Power Saver",
    balanced: "Balanced",
    performance: "Performance",
  };
  // When pinned to Auto, surface which concrete profile is currently active.
  let resolvedActive = $derived(
    energyMode === "auto" && $profile?.active
      ? t("settings.general.energy_auto_active", { profile: PROFILE_LABELS[$profile.active] })
      : null,
  );

  // About tab: surface the overall compute backend as a single status. The
  // engines almost always agree on Apple Silicon (both Metal); when they
  // diverge or any one falls back to CPU we surface the worst-case state so
  // the hint copy stays honest. Order of precedence: resolving < cpu < gpu.
  let backendOverall = $derived.by<BackendState>(() => {
    const states = [backend.stt.state, backend.llm.state];
    if (states.includes("resolving")) return "resolving";
    if (states.includes("cpu")) return "cpu";
    return "gpu";
  });

  let devices = $state<InputDeviceEntry[]>([]);

  onMount(async () => {
    // Only enumerate input devices if mic permission was already granted.
    // On macOS 14+ Core Audio HAL surfaces the TCC prompt the moment we
    // open the device list, even read-only — we don't want to flash that
    // dialog every time the user opens Settings.
    const mic = await lda.checkMicrophone().catch(() => "denied" as const);
    if (mic === "granted") {
      try {
        devices = await lda.listInputDevices();
      } catch {
        // not fatal — keep dropdown empty
      }
    }
  });

  // There is no real updater yet (XPLAT-2): rather than call the
  // always-`{ available: false }` stub and risk claiming "you are on the
  // latest version" when no check actually happened, this opens the GitHub
  // Releases page so the user can compare versions themselves.
  const RELEASES_URL = "https://github.com/fiorelorenzo/lirevo/releases/latest";

  async function checkUpdates() {
    try {
      await open(RELEASES_URL);
    } catch (e) {
      toastError(`${t("settings.about.check_updates_error")}: ${e}`);
    }
  }
</script>

<div class="h-full flex">
  <!-- Sidebar -->
  <nav class="w-44 bg-muted/30 backdrop-blur-xl border-r border-border p-3 flex flex-col gap-1">
    <!-- Passive top strip so the settings window can be dragged from the
         sidebar header, like a native macOS toolbar. -->
    <div data-tauri-drag-region class="h-6 -mx-3 -mt-3 mb-1 pointer-events-none"></div>
    <button
      class="text-left px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors mb-2"
      onclick={() => navigate("home")}
    >
      ← {t("settings.back_to_home")}
    </button>
    {#each TABS as tab (tab)}
      <button
        class={[
          "text-left px-3 py-2 rounded-md text-sm transition-colors",
          activeTab === tab ? "bg-primary text-primary-foreground" : "hover:bg-accent",
        ].join(" ")}
        onclick={() => (activeTab = tab)}
      >
        {t(`settings.tabs.${tab}`)}
      </button>
    {/each}
  </nav>

  <!-- Content -->
  <section class="flex-1 p-8 overflow-y-auto">
    {#if $settings && activeTab === "general"}
      <div class="space-y-8 max-w-lg">
        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t("settings.general.section")}
          </h2>
          <div
            class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden"
          >
            <div class="p-4 flex items-center justify-between gap-4">
              <Label class="shrink-0">{t("settings.general.language")}</Label>
              <Select.Root
                type="single"
                value={$settings.language}
                onValueChange={(v) => v && updateSettings({ language: v })}
              >
                <Select.Trigger class="w-56">
                  <span class="flex-1 min-w-0 truncate text-left">
                    {LANGUAGE_OPTIONS.find((o) => o.value === $settings.language)?.label ??
                      $settings.language}
                  </span>
                </Select.Trigger>
                <Select.Content>
                  {#each LANGUAGE_OPTIONS as opt (opt.value)}
                    <Select.Item value={opt.value}>{opt.label}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </div>

            <div class="p-4 flex items-center justify-between gap-4">
              <Label class="shrink-0">{t("settings.general.input_device")}</Label>
              <Select.Root
                type="single"
                value={$settings.inputDeviceName ?? "__default__"}
                onValueChange={(v) =>
                  updateSettings({ inputDeviceName: v === "__default__" ? null : (v ?? null) })}
                disabled={devices.length === 0}
              >
                <Select.Trigger class="w-56">
                  <span class="flex-1 min-w-0 truncate text-left">
                    {$settings.inputDeviceName ??
                      (devices.find((d) => d.isDefault)?.name
                        ? `${devices.find((d) => d.isDefault)?.name} (${t("settings.general.input_device_default")})`
                        : t("settings.general.input_device_default"))}
                  </span>
                </Select.Trigger>
                <Select.Content>
                  <Select.Item value="__default__">
                    {devices.find((d) => d.isDefault)?.name
                      ? `${devices.find((d) => d.isDefault)?.name} (${t("settings.general.input_device_default")})`
                      : t("settings.general.input_device_default")}
                  </Select.Item>
                  {#each devices as d (d.name)}
                    <Select.Item value={d.name}>
                      {d.name}{d.isDefault
                        ? ` (${t("settings.general.input_device_default")})`
                        : ""}
                    </Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </div>
            <div class="p-4 flex items-start justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.smart_mic_routing")}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t("settings.general.smart_mic_routing_helper")}
                </p>
              </div>
              <Switch
                checked={$settings.smartMicRouting}
                onCheckedChange={(v) => updateSettings({ smartMicRouting: v })}
              />
            </div>
            {#if $settings.smartMicRouting}
              <div class="p-4 flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <Label>{t("settings.general.backup_mic")}</Label>
                  <p class="text-xs text-muted-foreground mt-1">
                    {t("settings.general.backup_mic_helper")}
                  </p>
                </div>
                <Select.Root
                  type="single"
                  value={$settings.backupInputDevice ?? "__builtin__"}
                  onValueChange={(v) =>
                    updateSettings({ backupInputDevice: v === "__builtin__" ? null : (v ?? null) })}
                  disabled={devices.length === 0}
                >
                  <Select.Trigger class="w-56 shrink-0">
                    <span class="flex-1 min-w-0 truncate text-left">
                      {$settings.backupInputDevice ?? t("settings.general.backup_mic_auto")}
                    </span>
                  </Select.Trigger>
                  <Select.Content>
                    <Select.Item value="__builtin__">
                      {t("settings.general.backup_mic_auto")}
                    </Select.Item>
                    {#each devices as d (d.name)}
                      <Select.Item value={d.name}>{d.name}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
              </div>
            {/if}
          </div>
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-1">
            {t("settings.general.microphone.section")}
          </h2>
          <p class="text-xs text-muted-foreground mb-3">
            {t("settings.general.microphone.section_helper")}
          </p>
          <MicTest />
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t("settings.general.injection_section")}
          </h2>
          <div
            class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden"
          >
            <div class="p-4 space-y-3">
              <div class="flex items-center justify-between gap-4">
                <Label>{t("settings.general.paste_delay_ms")}</Label>
                <span class="text-xs text-muted-foreground tabular-nums"
                  >{$settings.pasteDelayMs} ms</span
                >
              </div>
              <Slider
                type="single"
                min={0}
                max={2000}
                step={10}
                value={$settings.pasteDelayMs}
                onValueChange={(v) =>
                  updateSettings({ pasteDelayMs: typeof v === "number" ? v : v[0] })}
              />
            </div>
          </div>
        </section>

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t("settings.general.app_section")}
          </h2>
          <div
            class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden"
          >
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.launch_at_login")}</Label>
              </div>
              <Switch
                checked={$settings.launchAtLogin}
                onCheckedChange={(v) => updateSettings({ launchAtLogin: v })}
              />
            </div>
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.start_minimized")}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t("settings.general.start_minimized_helper")}
                </p>
              </div>
              <Switch
                checked={$settings.startMinimized}
                onCheckedChange={(v) => updateSettings({ startMinimized: v })}
              />
            </div>
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.record_history")}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t("settings.general.record_history_helper")}
                </p>
              </div>
              <Switch
                checked={$settings.recordHistory}
                onCheckedChange={(v) => updateSettings({ recordHistory: v })}
              />
            </div>
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.style_learning_enabled")}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t("settings.general.style_learning_enabled_helper")}
                </p>
              </div>
              <Switch
                checked={$settings.styleLearningEnabled}
                onCheckedChange={(v) => updateSettings({ styleLearningEnabled: v })}
              />
            </div>
            <div class="p-4 flex items-start justify-between gap-4">
              <div class="min-w-0">
                <Label>{t("settings.general.energy")}</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {t("settings.general.energy_helper")}
                </p>
              </div>
              <div class="shrink-0 flex flex-col items-end gap-1.5">
                <Select.Root
                  type="single"
                  value={energyMode}
                  onValueChange={(v) => v && setProfileMode(v)}
                >
                  <Select.Trigger class="w-40">
                    <span class="flex-1 min-w-0 truncate text-left">
                      {ENERGY_OPTIONS.find((o) => o.value === energyMode)?.label ?? energyMode}
                    </span>
                  </Select.Trigger>
                  <Select.Content>
                    {#each ENERGY_OPTIONS as opt (opt.value)}
                      <Select.Item value={opt.value}>{opt.label}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
                {#if resolvedActive}
                  <span class="text-xs text-muted-foreground">{resolvedActive}</span>
                {/if}
              </div>
            </div>
          </div>
        </section>
      </div>
    {:else if $settings && activeTab === "models"}
      <div class="space-y-8 max-w-2xl">
        <ModelStatusPanel />

        <section>
          <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
            {t("settings.models.advanced_section")}
          </h2>
          <div
            class="rounded-xl border border-border bg-surface divide-y divide-border overflow-hidden"
          >
            <div class="p-4 flex items-center justify-between gap-4">
              <Label>{t("settings.models.llm_ctx_size")}</Label>
              <Input
                type="number"
                class="w-32"
                value={String($settings.llmCtxSize)}
                onchange={(e) => {
                  const n = Number((e.currentTarget as HTMLInputElement).value);
                  if (!Number.isNaN(n) && n >= 512 && n <= 32768) {
                    updateSettings({ llmCtxSize: n });
                  }
                }}
              />
            </div>
          </div>
        </section>
      </div>
    {:else if $settings && activeTab === "hotkey"}
      <div class="space-y-3 max-w-lg">
        <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-3">
          {t("settings.hotkey.label")}
        </h2>
        <div class="rounded-xl border border-border bg-surface overflow-hidden">
          <div class="p-4">
            <HotkeyRecorder
              spec={$settings.hotkey}
              mode={$settings.activationMode}
              onchange={(n) =>
                updateSettings({ hotkey: n.hotkey, activationMode: n.activationMode })}
            />
          </div>
        </div>
      </div>
    {:else if $settings && activeTab === "about"}
      {@const isGpu = backendOverall === "gpu"}
      {@const isResolving = backendOverall === "resolving"}
      <div class="space-y-6 max-w-lg">
        <div class="rounded-xl border border-border bg-surface p-5 space-y-1">
          <div class="font-semibold text-lg">Lirevo</div>
          <div class="text-sm text-muted-foreground tabular-nums">
            {t("settings.about.version")}: {$settings.appVersion}
          </div>
          <div class="text-sm text-muted-foreground">macOS · arm64</div>
          <div class="text-sm text-muted-foreground pt-1">
            {t("settings.about.model_stt")}: {STT_MODELS[0].displayName}
          </div>
          <div class="text-sm text-muted-foreground">
            {t("settings.about.model_llm")}: {CLEANUP_MODEL.displayName}
          </div>
        </div>

        <div
          class={[
            "backend-card relative overflow-hidden rounded-xl border bg-surface p-4 shadow-sm transition-colors",
            isGpu ? "border-primary/30" : isResolving ? "border-border" : "border-warning/40",
          ].join(" ")}
        >
          <div class="relative flex items-center gap-4">
            <div
              class={[
                "flex h-11 w-11 shrink-0 items-center justify-center rounded-lg",
                isGpu
                  ? "bg-primary/10 text-primary"
                  : isResolving
                    ? "bg-muted text-muted-foreground"
                    : "bg-warning/10 text-warning",
              ].join(" ")}
            >
              {#if isGpu}
                <Zap class="h-5 w-5" />
              {:else}
                <Cpu class={["h-5 w-5", isResolving ? "backend-pulse" : ""].join(" ")} />
              {/if}
            </div>

            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span
                  class="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground"
                >
                  {t("settings.about.backend")}
                </span>
                {#if isGpu}
                  <span
                    class="inline-flex items-center gap-1 rounded-full bg-primary/10 px-2 py-0.5 text-[10px] font-medium leading-none text-primary"
                  >
                    {t("settings.about.backend_gpu_hint")}
                  </span>
                {/if}
              </div>
              <div class="mt-0.5 truncate text-base font-semibold tabular-nums">
                {isResolving ? t("settings.about.backend_resolving") : backend.stt.label}
              </div>
              {#if !backend.unified && !isResolving}
                <!-- Engines disagree (rare): show both so the label isn't a lie. -->
                <div
                  class="mt-1.5 flex flex-wrap gap-x-4 gap-y-0.5 text-xs text-muted-foreground tabular-nums"
                >
                  <span
                    ><span class="text-muted-foreground/70">{t("settings.about.backend_stt")}:</span
                    >
                    {backend.stt.label}</span
                  >
                  <span
                    ><span class="text-muted-foreground/70">{t("settings.about.backend_llm")}:</span
                    >
                    {backend.llm.label}</span
                  >
                </div>
              {/if}
              <p class="mt-1 text-xs text-muted-foreground">
                {#if isResolving}
                  {t("settings.about.backend_resolving_hint")}
                {:else if isGpu}
                  {t("settings.about.backend_helper")}
                {:else}
                  <span class="text-warning">{t("settings.about.backend_cpu_hint")}</span>
                {/if}
              </p>
            </div>
          </div>
        </div>

        <div class="flex flex-wrap gap-3">
          <Button variant="outline" onclick={checkUpdates}>
            {t("settings.about.check_updates")}
          </Button>
          <Button variant="outline" onclick={() => navigate("wizard")}>
            {t("settings.about.rerun_wizard")}
          </Button>
        </div>

        <p class="text-xs text-muted-foreground">
          {t("settings.about.license")}
        </p>
      </div>
    {/if}
  </section>
</div>

<style>
  /* Backend card: only a subtle pulse on the transient resolving icon, gated on
     prefers-reduced-motion (see Logo.svelte / overlay for the same pattern). */
  .backend-card .backend-pulse {
    animation: backend-fade 1.6s var(--ease-in-out-soft) infinite;
  }

  @keyframes backend-fade {
    0%,
    100% {
      opacity: 0.45;
    }
    50% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .backend-card .backend-pulse {
      animation: none;
    }
  }
</style>
