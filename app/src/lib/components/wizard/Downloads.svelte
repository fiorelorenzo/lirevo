<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Progress } from "$lib/components/ui/progress";
  import { Check, Loader2, AlertCircle, Mic, Sparkles } from "@lucide/svelte";
  import { settings, updateSettings } from "$lib/stores/settings.svelte";
  import { wizardDownloadSelection } from "$lib/stores/wizardDownloads";
  import { progressFor } from "$lib/stores/downloads";
  import { lda, type LocalModel, type DownloadProgress } from "$lib/tauri";
  import { withErrorToast } from "$lib/stores/toasts";
  import { t } from "$lib/i18n";
  import { defaultStepState, type WizardStepState } from "./step-state";

  interface Props {
    onnext: () => void;
    nextState?: WizardStepState;
  }
  let { onnext, nextState = $bindable(defaultStepState()) }: Props = $props();

  let local = $state<LocalModel[]>([]);

  // STT id comes from settings (Language step persisted it); LLM id from the
  // sibling handoff store. Both are resolved before this step mounts.
  let sttId = $derived($settings?.sttModelId ?? null);
  let llmId = $derived($wizardDownloadSelection.llmId);

  // Reactive progress streams for each id, sourced from the global downloads
  // store's single `download:progress` listener (no second listener here).
  let sttProgress = $derived(sttId ? progressFor(sttId) : null);
  let llmProgress = $derived(llmId ? progressFor(llmId) : null);

  async function refreshLocal() {
    try {
      local = await lda.modelsListLocal();
    } catch {
      // Non-fatal — the path persist below simply no-ops until it succeeds.
    }
  }

  onMount(async () => {
    await refreshLocal();
  });

  function isComplete(p: DownloadProgress | null | undefined): boolean {
    return p?.state === "complete";
  }
  function isError(p: DownloadProgress | null | undefined): boolean {
    return p?.state === "error";
  }
  function isActive(p: DownloadProgress | null | undefined): boolean {
    return p?.state === "downloading" || p?.state === "queued" || p?.state === "verifying";
  }

  function fmtSize(bytes: number): string {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }
  function pct(p: DownloadProgress): number {
    return (p.bytesReceived / Math.max(1, p.bytesTotal)) * 100;
  }

  // When the LLM finishes, persist its local path so the engine knows which
  // weights to load — mirrors what the old Cleanup step did on `complete`.
  let llmPathPersisted = $state(false);
  $effect(() => {
    const p = $llmProgress;
    if (!llmId || llmPathPersisted) return;
    if (p?.state === "complete") {
      llmPathPersisted = true;
      void (async () => {
        await refreshLocal();
        const match = local.find((l) => l.id === llmId);
        if (match) await updateSettings({ llmModelPath: match.path });
      })();
    }
  });

  async function retrySTT() {
    if (!sttId) return;
    await withErrorToast(t("wizard.downloads.error"), () => lda.sttDownload(sttId));
  }
  async function retryLLM() {
    if (!llmId) return;
    await withErrorToast(t("wizard.downloads.error"), () => lda.modelsDownload(llmId));
  }

  // Both downloads must reach `complete` before the user can continue. The
  // LLM card is treated as already-done when no LLM was selected (recommended
  // id unavailable), so the wizard never wedges.
  let sttDone = $derived(isComplete($sttProgress));
  let llmDone = $derived(!llmId || isComplete($llmProgress));
  let bothDone = $derived(sttDone && llmDone);

  $effect(() => {
    nextState = { canNext: bothDone, onNextClick: onnext };
  });
</script>

{#snippet card(
  role: string,
  Icon: typeof Mic,
  progress: DownloadProgress | null | undefined,
  retry: () => void,
)}
  <div class="w-full rounded-xl border border-border bg-surface p-4 text-left space-y-3">
    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-3 min-w-0">
        <div
          class="flex h-9 w-9 items-center justify-center rounded-lg bg-muted text-muted-foreground shrink-0"
        >
          <Icon class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <div class="font-medium truncate">{role}</div>
        </div>
      </div>

      {#if isComplete(progress)}
        <span
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-success/10 text-success text-[11px] font-medium leading-none"
        >
          <Check class="h-3 w-3" />
          {t("wizard.downloads.complete")}
        </span>
      {:else if isError(progress)}
        <span
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-warning/10 text-warning text-[11px] font-medium leading-none"
        >
          <AlertCircle class="h-3 w-3" />
          {t("wizard.downloads.error")}
        </span>
      {:else}
        <span
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-muted text-muted-foreground text-[11px] font-medium leading-none"
        >
          <Loader2 class="h-3 w-3 animate-spin" />
          {t("wizard.downloads.downloading")}
        </span>
      {/if}
    </div>

    {#if isError(progress)}
      <div class="space-y-2">
        {#if progress?.errorMessage}
          <p class="text-xs text-warning break-words">{progress.errorMessage}</p>
        {/if}
        <Button variant="outline" size="sm" onclick={retry}>
          {t("wizard.downloads.retry")}
        </Button>
      </div>
    {:else}
      <div class="space-y-1">
        <Progress
          value={progress && isActive(progress) ? pct(progress) : isComplete(progress) ? 100 : 0}
          class="h-1.5"
        />
        <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
          {#if progress && progress.bytesTotal > 0}
            <span>{fmtSize(progress.bytesReceived)} / {fmtSize(progress.bytesTotal)}</span>
            <span>{Math.round(pct(progress))}%</span>
          {:else if isComplete(progress)}
            <span></span>
            <span>100%</span>
          {:else}
            <span>{t("wizard.downloads.downloading")}</span>
            <span></span>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/snippet}

<div class="max-w-md mx-auto flex flex-col gap-6">
  <div class="text-center space-y-2 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <h1 class="text-2xl font-semibold tracking-tight">{t("wizard.downloads.title")}</h1>
    <p class="text-sm text-muted-foreground">{t("wizard.downloads.body")}</p>
  </div>

  <div class="space-y-3 animate-in fade-in duration-500 delay-200">
    {@render card(t("wizard.downloads.dictation_label"), Mic, $sttProgress, retrySTT)}
    {#if llmId}
      {@render card(t("wizard.downloads.cleanup_label"), Sparkles, $llmProgress, retryLLM)}
    {/if}
  </div>

  {#if !bothDone}
    <p class="text-center text-xs text-muted-foreground animate-in fade-in duration-500">
      {t("wizard.downloads.downloading")}
    </p>
  {/if}
</div>
