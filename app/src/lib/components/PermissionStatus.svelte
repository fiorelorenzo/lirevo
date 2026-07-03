<script lang="ts">
  import { Check, AlertCircle, Loader2, HelpCircle } from "@lucide/svelte";

  interface Props {
    /** `null` only during the initial fetch (before the first IPC reply). */
    status: "granted" | "denied" | "not_determined" | null;
    granted_label?: string;
    denied_label?: string;
    /** Shown when status is `not_determined` (process never requested it yet). */
    not_determined_label?: string;
  }
  let {
    status,
    granted_label = "Granted",
    denied_label = "Not granted yet",
    not_determined_label = "Not requested yet",
  }: Props = $props();
</script>

<div
  class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium transition-colors duration-200
    {status === 'granted'
    ? 'bg-success/10 text-success'
    : status === 'denied'
      ? 'bg-warning/10 text-warning'
      : status === 'not_determined'
        ? 'bg-muted text-muted-foreground'
        : 'bg-muted text-muted-foreground'}"
>
  {#if status === null}
    <Loader2 class="h-4 w-4 animate-spin" />
    <span>Checking…</span>
  {:else if status === "granted"}
    <Check class="h-4 w-4" />
    <span>{granted_label}</span>
  {:else if status === "denied"}
    <AlertCircle class="h-4 w-4" />
    <span>{denied_label}</span>
  {:else}
    <HelpCircle class="h-4 w-4" />
    <span>{not_determined_label}</span>
  {/if}
</div>
