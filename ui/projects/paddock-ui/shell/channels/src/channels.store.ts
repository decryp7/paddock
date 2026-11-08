import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { MAX_CHANNELS, type ChannelConfig, type ChannelsEvent, type ChannelsView, type TestPhase } from '../generated/channels';
import { ChannelsBackend, type Unlisten } from './backend';
import { channelsMessage } from './wire';

/** What a running test is doing, as the engine last said (phase-labelled; the attempt only goes up). */
export interface TestProgress {
  readonly attempt: number;
  readonly of: number;
  readonly phase: TestPhase;
  readonly waitSecs: number | null;
}

/** Where a test send of one channel is. The engine decides the outcome; a running test is never timed out (rule 12). */
export type TestState =
  | { readonly state: 'running'; readonly testId: number | null; readonly progress: TestProgress | null }
  | { readonly state: 'delivered'; readonly attempts: number }
  | { readonly state: 'failed'; readonly attempts: number; readonly message: string };

/** Where the last save (or removal) is: one union, never separate flags. */
export type SaveStatus = 'idle' | 'saving' | { readonly error: string };

interface ChannelsState {
  /** What the engine last said; null until the first answer. */
  view: ChannelsView | null;
  saveStatus: SaveStatus;
  /** The latest test of each channel id. */
  tests: Readonly<Record<string, TestState>>;
}

/**
 * The notification channels as one window sees them (rule 34: the engine is the single source of truth). The store holds the
 * view the engine reported, saves and removes channels, starts tests, which return at once, and folds the outcomes that arrive
 * on `paddock://channels` into the per-channel test state. A refusal of a save is kept as the engine worded it. Secrets never
 * pass through here: they go from the secret dialog's window straight to the backend.
 */
export const ChannelsStore = signalStore(
  withState<ChannelsState>({ view: null, saveStatus: 'idle', tests: {} }),
  // A failed read retries itself; a test event while in error reads the view again (ADR 0058).
  withRequestStatus({ retry: true }),
  withComputed(({ view, saveStatus }) => ({
    channels: computed(() => view()?.settings.channels ?? []),
    /** Whether channel `id` has a secret stored. */
    secretSet: computed(() => view()?.secretSet ?? {}),
    atLimit: computed(() => (view()?.settings.channels.length ?? 0) >= MAX_CHANNELS),
    saving: computed(() => saveStatus() === 'saving'),
    saveError: computed(() => {
      const s = saveStatus();
      return typeof s === 'object' ? s.error : null;
    }),
  })),
  withMethods((store, backend = inject(ChannelsBackend)) => {
    const setTest = (id: string, test: TestState): void => patchState(store, { tests: { ...store.tests(), [id]: test } });
    const dropTest = (id: string): void => {
      const { [id]: _gone, ...rest } = store.tests();
      patchState(store, { tests: rest });
    };
    const reads = store._reads.define({
      read: () => backend.get(),
      apply: (view: ChannelsView) => patchState(store, { view }),
      message: (e) => channelsMessage(e, $localize`:@@pd.channels.load-failed:The notification channels could not be read.`),
      // A refresh keeps the list on screen; only the first ask shows as loading.
      quiet: () => store.view() !== null,
    });
    const refresh = async (): Promise<void> => {
      try {
        patchState(store, { view: await backend.get() });
        reads.received();
      } catch {
        // The view on screen stays; a failed refresh is not worth hiding it.
      }
    };
    const save = async (channels: readonly ChannelConfig[]): Promise<boolean> => {
      patchState(store, { saveStatus: 'saving' });
      try {
        const view = await backend.save({ channels: [...channels] });
        patchState(store, { view, saveStatus: 'idle' });
        reads.received();
        return true;
      } catch (e) {
        patchState(store, { saveStatus: { error: channelsMessage(e, $localize`:@@pd.channels.save-failed:The channels could not be saved.`) } });
        return false;
      }
    };
    return {
      /** Asks the engine for the channels (also the Retry of a failed ask). */
      async load(): Promise<void> {
        await reads.load();
      },
      /** Re-reads the view quietly (after the secret dialog stored or the engine changed something). */
      refresh,
      /** Adds the channel, or replaces the one of the same id. Resolves true when the engine stored it; otherwise `saveError` says why. */
      upsert(channel: ChannelConfig): Promise<boolean> {
        const list = store.channels();
        const at = list.findIndex((c) => c.id === channel.id);
        return save(at < 0 ? [...list, channel] : list.map((c) => (c.id === channel.id ? channel : c)));
      },
      /** Removes a channel: saves without it, then deletes its secret. */
      async remove(id: string): Promise<boolean> {
        const ok = await save(store.channels().filter((c) => c.id !== id));
        if (!ok) return false;
        dropTest(id);
        try {
          await backend.clearSecret(id);
        } catch (e) {
          console.warn(`[paddock-channels] the secret of ${id} could not be deleted: ${errorMessage(e, 'unknown error')}`);
        }
        await refresh();
        return true;
      },
      /** Deletes the stored secret of a channel. Resolves false (with `saveError` set) when the engine refused. */
      async clearSecret(id: string): Promise<boolean> {
        try {
          await backend.clearSecret(id);
        } catch (e) {
          patchState(store, { saveStatus: { error: channelsMessage(e, $localize`:@@pd.channels.clear-failed:The secret could not be removed.`) } });
          return false;
        }
        await refresh();
        return true;
      },
      /** Sends a test with `channel` (the draft or a saved one); the outcome arrives as an event and is never timed out. */
      async test(channel: ChannelConfig): Promise<void> {
        if (store.tests()[channel.id]?.state === 'running') return;
        setTest(channel.id, { state: 'running', testId: null, progress: null });
        try {
          const testId = await backend.test(channel);
          const now = store.tests()[channel.id];
          // The outcome can arrive before the answer; then it already stands.
          if (now?.state === 'running') setTest(channel.id, { state: 'running', testId: now.testId ?? testId, progress: now.progress });
        } catch (e) {
          setTest(channel.id, { state: 'failed', attempts: 0, message: channelsMessage(e, $localize`:@@pd.channels.test-refused:The test could not be started.`) });
        }
      },
      /** Forgets the shown result of a channel's test. */
      clearTest(id: string): void {
        if (store.tests()[id]?.state !== 'running') dropTest(id);
      },
      /** Hides the last save failure. */
      dismissSaveError(): void {
        patchState(store, { saveStatus: 'idle' });
      },
      /** An event from the engine: progress or the outcome of a test, matched by channel and test id (a stale one is ignored). */
      _event(event: ChannelsEvent): void {
        // The engine answers again: while the view is in error, read it again.
        reads.event();
        const now = store.tests()[event.channelId];
        if (now?.state !== 'running' || (now.testId !== null && now.testId !== event.testId)) return;
        if (event.kind === 'testProgress') {
          // The attempt never goes backwards: a late event of an earlier attempt changes nothing.
          if (now.progress && event.attempt < now.progress.attempt) return;
          const progress = { attempt: event.attempt, of: event.of, phase: event.phase, waitSecs: event.waitSecs };
          setTest(event.channelId, { state: 'running', testId: event.testId, progress });
          return;
        }
        setTest(
          event.channelId,
          event.ok
            ? { state: 'delivered', attempts: event.attempts }
            : { state: 'failed', attempts: event.attempts, message: event.message ?? $localize`:@@pd.channels.test-failed:The test did not get through.` },
        );
      },
    };
  }),
  withHooks((store, backend = inject(ChannelsBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: an outcome can only follow a test this window started.
        void backend
          .listen((e) => store._event(e))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-channels] cannot listen for test results: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type ChannelsStoreType = InstanceType<typeof ChannelsStore>;
