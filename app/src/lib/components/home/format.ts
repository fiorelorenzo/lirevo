/** Format a duration in milliseconds as a short human string ("1.0s", "340ms"). */
export function formatMs(ms: number | null | undefined): string {
  if (ms == null) return '—';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

/** Relative time from a unix-millis timestamp: "just now" / "5m" / "2h" / a date. */
export function formatRelative(createdAt: number, now: number = Date.now()): string {
  const diff = now - createdAt;
  if (diff < 0) return 'just now';
  const sec = Math.floor(diff / 1000);
  if (sec < 45) return 'just now';
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  const day = Math.floor(hr / 24);
  if (day < 7) return `${day}d`;
  return new Date(createdAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Absolute, locale-aware timestamp for the detail view. */
export function formatAbsolute(createdAt: number): string {
  return new Date(createdAt).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}
