<script lang="ts">
  import { Check, AlertCircle, Loader2 } from '@lucide/svelte';

  interface Props {
    status: 'granted' | 'denied' | 'not_determined' | null;
    granted_label?: string;
    denied_label?: string;
  }
  let { status, granted_label = 'Granted', denied_label = 'Not granted yet' }: Props = $props();
</script>

<div
  class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium transition-colors duration-200
    {status === 'granted' ? 'bg-success/10 text-success' :
     status === 'denied'  ? 'bg-warning/10 text-warning' :
                            'bg-muted text-muted-foreground'}"
>
  {#if status === 'granted'}
    <Check class="h-4 w-4" />
    <span>{granted_label}</span>
  {:else if status === 'denied'}
    <AlertCircle class="h-4 w-4" />
    <span>{denied_label}</span>
  {:else}
    <Loader2 class="h-4 w-4 animate-spin" />
    <span>Checking...</span>
  {/if}
</div>
