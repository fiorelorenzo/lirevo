import { writable, type Readable } from 'svelte/store';
import { lda, type ProfileStatus } from '../tauri';

const _profile = writable<ProfileStatus | null>(null);

/** Live energy-profile status. `null` until the first `profile:changed`
 *  event lands or the initial `profile_get` succeeds. */
export const profile: Readable<ProfileStatus | null> = { subscribe: _profile.subscribe };

// The backend emits `profile:changed` on every change; that is the source
// of truth for live updates.
void lda.onProfileChanged((s) => _profile.set(s));

// Seed the initial value. `profile_get` can REJECT in the first few
// milliseconds after launch — the selector is created slightly after
// startup — so retry a couple of times with a short delay and then give up
// quietly. Live updates still arrive via the event listener above.
async function seed(attemptsLeft = 3): Promise<void> {
  try {
    _profile.set(await lda.profileGet());
  } catch {
    if (attemptsLeft > 1) {
      setTimeout(() => void seed(attemptsLeft - 1), 500);
    }
    // else: swallow — the event listener will fill the store on next change.
  }
}
void seed();

/** Pin or unpin the energy profile. The resulting state comes back via the
 *  `profile:changed` event; we optimistically reflect `mode` immediately for
 *  snappiness and let the event confirm (and resolve `active`). */
export async function setProfileMode(mode: string): Promise<void> {
  _profile.update((p) =>
    p ? { ...p, mode: mode as ProfileStatus['mode'] } : p,
  );
  await lda.profileSetMode(mode);
}
