import { writable } from "svelte/store";
import { lda, type DictationSummary } from "../tauri";

const PAGE = 50;

interface HistoryState {
  items: DictationSummary[];
  hasMore: boolean;
  loading: boolean;
}

const _store = writable<HistoryState>({ items: [], hasMore: false, loading: false });

let items: DictationSummary[] = [];
let hasMore = false;
let loading = false;

function publish() {
  _store.set({ items, hasMore, loading });
}

/** Fetch the next page (offset = current item count) and append it. */
async function loadMore(): Promise<void> {
  if (loading) return;
  loading = true;
  publish();
  try {
    const page = await lda.historyList(PAGE, items.length);
    items = [...items, ...page];
    hasMore = page.length === PAGE;
  } finally {
    loading = false;
    publish();
  }
}

/** Clear local state and load the first page from scratch. */
async function load(): Promise<void> {
  items = [];
  hasMore = false;
  await loadMore();
}

async function removeOne(id: number): Promise<void> {
  await lda.historyDelete(id);
  items = items.filter((i) => i.id !== id);
  publish();
}

async function clearAll(): Promise<void> {
  await lda.historyClear();
  items = [];
  hasMore = false;
  publish();
}

/** Prepend a freshly-saved dictation, de-duplicating by id. */
function prepend(summary: DictationSummary) {
  if (items.some((i) => i.id === summary.id)) return;
  items = [summary, ...items];
  publish();
}

// Module-level singleton subscription: new dictations arrive via the backend
// `dictation:saved` event and are prepended live, no reload needed.
void lda.onDictationSaved((s) => prepend(s));

export const dictationHistory = {
  subscribe: _store.subscribe,
  load,
  loadMore,
  removeOne,
  clearAll,
  reset: load,
};
