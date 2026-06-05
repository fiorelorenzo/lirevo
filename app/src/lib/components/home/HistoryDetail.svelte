<script lang="ts">
  import {
    Mic,
    WandSparkles,
    ArrowRight,
    Cpu,
    Clock,
    Languages,
    AppWindow,
    TriangleAlert,
    Bluetooth,
  } from '@lucide/svelte';
  import { lda, type Dictation } from '$lib/tauri';
  import Spinner from '$lib/components/Spinner.svelte';
  import { formatMs, formatAbsolute } from './format';

  interface Props {
    id: number;
  }
  let { id }: Props = $props();

  let detail = $state<Dictation | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Re-fetch whenever the selected id changes (the parent reuses one mounted
  // instance per selection, so a $effect on `id` is what drives the reload).
  $effect(() => {
    const wanted = id;
    loading = true;
    error = null;
    detail = null;
    void lda
      .historyGet(wanted)
      .then((d) => {
        // Guard against an out-of-order resolve when id changed mid-flight.
        if (wanted !== id) return;
        detail = d;
        loading = false;
      })
      .catch((e) => {
        if (wanted !== id) return;
        error = String(e);
        loading = false;
      });
  });

  let cleanupSkipped = $derived(detail?.cleanupStatus === 'skipped');
  let cleanupFailed = $derived(detail?.cleanupStatus === 'failed');
</script>

<div>
  {#if loading}
    <div class="flex items-center justify-center py-6">
      <Spinner size="sm" label="Loading transcript" />
    </div>
  {:else if error}
    <div class="flex items-center gap-2 text-sm text-destructive">
      <TriangleAlert class="h-4 w-4 shrink-0" />
      <span>Could not load this dictation.</span>
    </div>
  {:else if detail}
    <div class="space-y-4">
      <!-- Transcription step -->
      <section class="space-y-2">
        <div class="flex items-center gap-2 text-xs font-semibold text-muted-foreground">
          <Mic class="h-3.5 w-3.5 text-primary" />
          <span class="uppercase tracking-wide">Transcription</span>
        </div>
        <div class="flex items-start gap-3 rounded-md bg-muted/30 p-3">
          <span class="shrink-0 pt-0.5 text-xs tabular-nums text-muted-foreground">
            {formatMs(detail.audioMs)}
          </span>
          <ArrowRight class="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground/60" />
          <p class="whitespace-pre-wrap break-words text-sm text-foreground">
            {detail.rawText || '(no transcript)'}
          </p>
        </div>
        <div class="flex items-center gap-3 pl-1 text-[11px] text-muted-foreground">
          <span class="inline-flex items-center gap-1">
            <Cpu class="h-3 w-3" />{detail.sttModel}
          </span>
          <span class="inline-flex items-center gap-1 tabular-nums">
            <Clock class="h-3 w-3" />{formatMs(detail.sttMs)}
          </span>
        </div>
      </section>

      <!-- Cleanup step -->
      <section class="space-y-2">
        <div class="flex items-center gap-2 text-xs font-semibold text-muted-foreground">
          <WandSparkles class="h-3.5 w-3.5 text-primary" />
          <span class="uppercase tracking-wide">Cleanup</span>
        </div>
        {#if cleanupSkipped}
          <p class="rounded-md bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
            Skipped (dictation-only)
          </p>
        {:else if cleanupFailed}
          <div
            class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm"
          >
            <TriangleAlert class="mt-0.5 h-4 w-4 shrink-0 text-warning" />
            <div class="min-w-0">
              <p class="font-medium">Cleanup failed</p>
              {#if detail.cleanupError}
                <p class="mt-0.5 break-words font-mono text-xs text-muted-foreground">
                  {detail.cleanupError}
                </p>
              {/if}
            </div>
          </div>
        {:else}
          <div class="flex items-start gap-3 rounded-md bg-muted/30 p-3">
            <p class="min-w-0 flex-1 whitespace-pre-wrap break-words text-xs text-muted-foreground line-clamp-3">
              {detail.rawText}
            </p>
            <ArrowRight class="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground/60" />
            <p class="min-w-0 flex-1 whitespace-pre-wrap break-words text-sm text-foreground">
              {detail.cleanedText || '(empty)'}
            </p>
          </div>
        {/if}
        {#if !cleanupSkipped}
          <div class="flex items-center gap-3 pl-1 text-[11px] text-muted-foreground">
            <span class="inline-flex items-center gap-1">
              <Cpu class="h-3 w-3" />{detail.llmModel ?? '—'}
            </span>
            <span class="inline-flex items-center gap-1 tabular-nums">
              <Clock class="h-3 w-3" />{formatMs(detail.cleanMs)}
            </span>
          </div>
        {/if}
      </section>

      <!-- Injection + metadata -->
      <section
        class="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-t border-border/60 pt-3 text-[11px] text-muted-foreground"
      >
        <span class="inline-flex items-center gap-1 font-medium text-foreground/80">
          {detail.injectMethod}
        </span>
        {#if detail.targetApp}
          <span class="inline-flex items-center gap-1">
            <AppWindow class="h-3 w-3" />{detail.targetApp}
          </span>
        {/if}
        {#if detail.inputDevice}
          <span class="inline-flex items-center gap-1">
            {#if detail.smartRoutingApplied}
              <Bluetooth class="h-3 w-3" />
            {:else}
              <Mic class="h-3 w-3" />
            {/if}
            {detail.inputDevice}
          </span>
        {/if}
        {#if detail.smartRoutingApplied}
          <span
            class="inline-flex items-center gap-1 rounded-full bg-primary/10 px-1.5 py-0.5 text-primary leading-none"
          >
            smart routing
          </span>
        {/if}
        <span class="inline-flex items-center gap-1 tabular-nums">
          inject {formatMs(detail.injectMs)}
        </span>
        <span class="inline-flex items-center gap-1 tabular-nums">
          total {formatMs(detail.totalMs)}
        </span>
        <span class="ml-auto inline-flex items-center gap-2">
          {#if detail.language}
            <span class="inline-flex items-center gap-1">
              <Languages class="h-3 w-3" />{detail.language}
            </span>
          {/if}
          <span class="inline-flex items-center gap-1">
            <Clock class="h-3 w-3" />{formatAbsolute(detail.createdAt)}
          </span>
        </span>
      </section>
    </div>
  {/if}
</div>
