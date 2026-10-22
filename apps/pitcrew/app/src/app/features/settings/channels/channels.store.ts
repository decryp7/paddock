import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { ChannelConfig, ChannelSettings } from '../../../core/api.types';
import { BACKEND } from '../../../core/backend';
import { toCommandError } from '../../../core/command-error';
import { idle, pending, settle, type Loadable } from '../../../state/loadable';
import { secretKeyFor, withSecretKey } from './channels-model';

interface ChannelsState {
  channels: Loadable<ChannelSettings>;
  /** Whether the OS keychain holds a secret, by channel id. */
  secrets: Record<string, boolean>;
  saving: boolean;
}

/** The result of a test message. */
export interface TestResult {
  ok: boolean;
  message: string;
}

/** Notification channels and their secrets. Secrets are written to the keychain and never held here. */
export const ChannelsStore = signalStore(
  { providedIn: 'root' },
  withState<ChannelsState>({ channels: idle<ChannelSettings>(), secrets: {}, saving: false }),
  withComputed(({ channels }) => ({
    list: computed(() => channels().value?.channels ?? []),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    const probe = async (id: string): Promise<void> => {
      let has = false;
      try {
        has = await backend.hasChannelSecret(secretKeyFor(id));
      } catch {
        has = false;
      }
      patchState(store, (s) => ({ secrets: { ...s.secrets, [id]: has } }));
    };
    const load = async (): Promise<void> => {
      patchState(store, { channels: pending(store.channels()) });
      const next = await settle(store.channels(), () => backend.channels());
      patchState(store, { channels: next });
      await Promise.all((next.value?.channels ?? []).map((c) => probe(c.id)));
    };
    /** Runs `fn` as a save; resolves with the message to show inline, or null on success. */
    const guarded = async (fn: () => Promise<void>): Promise<string | null> => {
      patchState(store, { saving: true });
      try {
        await fn();
        return null;
      } catch (e) {
        return toCommandError(e).message;
      } finally {
        patchState(store, { saving: false });
      }
    };
    return {
      load,
      hasSecret: (id: string): boolean => store.secrets()[id] ?? false,
      /** Stores `secret` (when given) and saves the channel, adding it when new. */
      saveChannel(channel: ChannelConfig, secret: string): Promise<string | null> {
        return guarded(async () => {
          if (secret) {
            await backend.setChannelSecret(secretKeyFor(channel.id), secret);
            patchState(store, (s) => ({ secrets: { ...s.secrets, [channel.id]: true } }));
          }
          const saved = withSecretKey(channel, store.secrets()[channel.id] ?? false);
          const current = store.list();
          const channels = current.some((c) => c.id === saved.id) ? current.map((c) => (c.id === saved.id ? saved : c)) : [...current, saved];
          await backend.saveChannels({ channels });
          await load();
        });
      },
      /** Removes the stored secret of a channel. */
      clearSecret(id: string): Promise<string | null> {
        return guarded(async () => {
          await backend.clearChannelSecret(secretKeyFor(id));
          patchState(store, (s) => ({ secrets: { ...s.secrets, [id]: false } }));
        });
      },
      /** Deletes a channel and its secret. */
      remove(id: string): Promise<string | null> {
        return guarded(async () => {
          await backend.clearChannelSecret(secretKeyFor(id));
          await backend.saveChannels({ channels: store.list().filter((c) => c.id !== id) });
          await load();
        });
      },
      async test(id: string): Promise<TestResult> {
        try {
          return { ok: true, message: await backend.testChannel(id) };
        } catch (e) {
          return { ok: false, message: toCommandError(e).message };
        }
      },
    };
  }),
);
