import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { Avatar } from '../core/api.types';
import { BACKEND } from '../core/backend';

/** What an identity is looked up by: a watch, or a catalogue preset. */
export type AvatarRef = { watch: string } | { preset: string };

const keyOf = (r: AvatarRef): string => ('watch' in r ? `w:${r.watch}` : `p:${r.preset}`);

interface AvatarsState {
  /** Data URLs by identity; null means the engine has no picture (the monogram shows). Absent means not asked yet. */
  images: Record<string, string | null>;
}

/**
 * The pictures the engine cached for watches and presets. Each identity is asked for once per session; a failure
 * is the same as "no picture", because an avatar is decoration and the name is always shown next to it.
 */
export const AvatarsStore = signalStore(
  { providedIn: 'root' },
  withState<AvatarsState>({ images: {} }),
  withMethods((store) => {
    const backend = inject(BACKEND, { optional: true });
    const asked = new Set<string>();

    return {
      /** Starts the lookup for an identity unless it was already asked for. */
      request(ref: AvatarRef): void {
        const key = keyOf(ref);
        if (!backend || asked.has(key)) return;
        asked.add(key);
        const lookup: Promise<Avatar | null> = 'watch' in ref ? backend.watchAvatar(ref.watch) : backend.presetAvatar(ref.preset);
        void lookup
          .catch(() => null)
          .then((a) => patchState(store, (s) => ({ images: { ...s.images, [key]: a?.dataUrl ?? null } })));
      },
      /** The data URL for an identity, or null while unknown or when there is none. */
      urlOf(ref: AvatarRef): string | null {
        return store.images()[keyOf(ref)] ?? null;
      },
    };
  }),
);
