<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { FolderOpen } from "@lucide/svelte";
  import { lda, type FileFilter } from "$lib/tauri";
  import { withErrorToast } from "$lib/stores/toasts";
  import { t } from "$lib/i18n";

  interface Props {
    value: string | null;
    filters?: FileFilter[];
    onpick?: (path: string) => void;
    placeholder?: string;
  }
  let {
    value = $bindable(null),
    filters,
    onpick,
    placeholder = "No file selected",
  }: Props = $props();

  async function pick() {
    const picked = await withErrorToast(t("common.error.file_picker"), () =>
      lda.pickFile(filters ?? []),
    );
    if (picked) {
      value = picked;
      onpick?.(picked);
    }
  }
</script>

<div class="flex items-center gap-2">
  <Input value={value ?? ""} readonly {placeholder} class="font-mono text-xs flex-1" />
  <Button variant="outline" onclick={pick}>
    <FolderOpen class="h-4 w-4 mr-1" />
    Browse...
  </Button>
</div>
