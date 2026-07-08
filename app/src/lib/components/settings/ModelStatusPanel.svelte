<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Check, Download, RotateCw } from "@lucide/svelte";
  import { t } from "$lib/i18n";
  import { lda, type LocalModel } from "$lib/tauri";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { progressFor } from "$lib/stores/downloads";
  import { withErrorToast } from "$lib/stores/toasts";
  import {
    STT_MODELS,
    CLEANUP_MODEL,
    PARAKEET_MODEL_ID,
    PARAKEET_FILENAME,
    CLEANUP_MODEL_ID,
    formatSize,
  } from "$lib/models/catalog";
  import { modelInstallState } from "./model-status";

  let local = $state<LocalModel[]>([]);
  let unlisten: UnlistenFn | null = null;

  const stt = STT_MODELS[0];

  // Store auto-subscription (`$store`) only works on variables declared at
  // the component's top level (see wizard/Downloads.svelte for the same
  // pattern) — so the progress streams are unwrapped here, not inside the
  // `rows` array or an `{@each}`-scoped `{@const}`.
  let sttProgress = $derived(progressFor(PARAKEET_MODEL_ID));
  let cleanupProgress = $derived(progressFor(CLEANUP_MODEL_ID));

  const rows = $derived([
    {
      role: t("settings.models.role_dictation"),
      name: stt.displayName,
      sizeBytes: stt.sizeBytes,
      progress: $sttProgress,
      state: modelInstallState(local, { filename: PARAKEET_FILENAME }),
      repair: () => lda.sttDownload(PARAKEET_MODEL_ID),
    },
    {
      role: t("settings.models.role_cleanup"),
      name: CLEANUP_MODEL.displayName,
      sizeBytes: CLEANUP_MODEL.sizeBytes,
      progress: $cleanupProgress,
      state: modelInstallState(local, { id: CLEANUP_MODEL_ID }),
      repair: () => lda.modelsDownload(CLEANUP_MODEL_ID),
    },
  ]);

  async function refresh() {
    const r = await withErrorToast(t("settings.models.error.refresh"), () => lda.modelsListLocal());
    if (r !== null) local = r;
  }

  async function repair(row: (typeof rows)[number]) {
    await withErrorToast(t("settings.models.repair_failed"), row.repair);
  }

  onMount(async () => {
    await refresh();
    unlisten = await lda.onDownloadProgress(async (p) => {
      if (p.state === "complete") {
        await refresh();
        // Reload so a freshly (re)downloaded model gets picked up by the engine.
        await lda
          .reloadModels()
          .catch((e) => console.warn("reloadModels after download failed", e));
      }
    });
  });
  onDestroy(() => unlisten?.());
</script>

<section class="space-y-2">
  <h2 class="text-xs font-semibold tracking-wide uppercase text-muted-foreground mb-1">
    {t("settings.models.section")}
  </h2>
  {#each rows as row (row.name)}
    {@const active =
      row.progress !== undefined &&
      (row.progress.state === "downloading" ||
        row.progress.state === "verifying" ||
        row.progress.state === "queued")}
    <div class="w-full p-4 bg-surface border border-border rounded-lg">
      <div class="flex items-start gap-4">
        <div class="flex-1 min-w-0">
          <div class="flex items-baseline gap-2 flex-wrap">
            <span class="text-xs uppercase tracking-wide text-muted-foreground">{row.role}</span>
            <span class="font-medium">{row.name}</span>
            <span class="text-xs text-muted-foreground tabular-nums">
              {formatSize(row.state.sizeBytes ?? row.sizeBytes)}
            </span>
          </div>
          {#if active && row.progress}
            <div class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-muted">
              <div
                class="h-full bg-primary transition-[width] duration-150"
                style={`width:${row.progress.bytesTotal ? Math.round((row.progress.bytesReceived / row.progress.bytesTotal) * 100) : 0}%`}
              ></div>
            </div>
          {/if}
        </div>
        <div class="shrink-0 flex items-center gap-2">
          {#if active}
            <span class="text-xs text-muted-foreground">{t("wizard.downloads.downloading")}</span>
          {:else if row.state.installed}
            <span
              class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 text-primary text-xs font-medium"
            >
              <Check class="h-3 w-3" />
              {t("settings.models.installed_badge")}
            </span>
            <Button variant="outline" size="sm" onclick={() => repair(row)}>
              <RotateCw class="h-3.5 w-3.5 mr-1" />
              {t("settings.models.repair_button")}
            </Button>
          {:else}
            <Button variant="outline" size="sm" onclick={() => repair(row)}>
              <Download class="h-3.5 w-3.5 mr-1" />
              {t("settings.models.download_button")}
            </Button>
          {/if}
        </div>
      </div>
    </div>
  {/each}
</section>
