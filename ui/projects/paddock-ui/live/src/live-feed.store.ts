import { computed, inject } from '@angular/core';
import { patchState, signalStoreFeature, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { foldFrame, initialFolded, parseFrame, type FoldOutcome, type FrameSource, type LiveFrame, type LiveLink } from './fold';
import { LIVE_TRANSPORT, type Unlisten } from './live-transport';

/** What a feed needs to fold: its channel, the state before the first snapshot, and the fold of one delta. */
export interface LiveFeedConfig<S, D> {
  /** The feed's name (`paddock_live::Channel`), registered in Rust with `paddock_live::tauri::register`. */
  readonly channel: string;
  /** What the window shows before the first snapshot. */
  readonly empty: S;
  /** Folds one delta into the state, returning the new state (pure; the same fold as Rust's `LiveState::apply`). */
  readonly apply: (state: S, delta: D) => S;
}

/** The state slice the feature adds. */
export interface LiveFeedState<S> {
  /** The folded state. */
  live: S;
  /** The sequence number the folded state stands for. */
  liveSeq: number;
  /** The link as Rust reports it. */
  liveLink: LiveLink;
}

/**
 * The SignalStore feature of a pushed feed (plan X29): the window holds what Rust pushes, folded with `apply`, and never asks
 * for it again except to repair itself.
 *
 * - `connectLive()` listens first, then fetches the snapshot, so nothing between is lost; the request status (rule 30) says
 *   where that first load is (`idle | loading | loaded | { error }`, `isLoading`, `requestError`).
 * - Frames fold in order. A delta that skips numbers (`gap`) triggers one `resyncLive()`; a duplicate or an older fetched
 *   snapshot is ignored; a pushed snapshot (Rust reseeded) replaces the state.
 * - `live` is the state, `liveSeq` its number, `liveLink` the connection to the source, with `liveConnected` and `liveStale`
 *   (anything but connected: show the last known state as stale, never as current).
 * - `disconnectLive()` and the store's destruction stop listening.
 *
 * Reads of signals happen before the first `await` (rule 29). Provide the store per window; the transport comes from
 * `LIVE_TRANSPORT`.
 */
export function withLiveFeed<S extends object, D>(config: LiveFeedConfig<S, D>) {
  const start = initialFolded(config.empty);
  return signalStoreFeature(
    withState<LiveFeedState<S>>({ live: start.state, liveSeq: start.seq, liveLink: start.link }),
    withRequestStatus({
      retry: false,
      why: 'not adopted yet (ADR 0058 rollout step 3): a gap or Retry resyncs, and liveLink shows the link state',
      until: '2026-12-31',
    }),
    withComputed(({ liveLink }) => ({
      /** The link is up and the state is live. */
      liveConnected: computed(() => liveLink().state === 'connected'),
      /** The state shown is the last known one: connecting, reconnecting, down or stopped. */
      liveStale: computed(() => liveLink().state !== 'connected'),
    })),
    withMethods((store, transport = inject(LIVE_TRANSPORT)) => {
      let unlisten: Unlisten | null = null;
      let connecting: Promise<void> | null = null;
      let fetching: Promise<void> | null = null;
      /** Counts disconnects, so a listen that finishes after one is undone. */
      let generation = 0;

      const take = (frame: LiveFrame<S, D>, source: FrameSource): FoldOutcome => {
        const cur = { state: store.live(), seq: store.liveSeq(), link: store.liveLink() };
        const { folded, outcome } = foldFrame(cur, frame, config.apply, source);
        if (outcome === 'applied') patchState(store, { live: folded.state, liveSeq: folded.seq, liveLink: folded.link });
        return outcome;
      };

      /** Fetches the snapshot; concurrent calls share one request. */
      const resyncLive = (): Promise<void> => {
        if (fetching) return fetching;
        patchState(store, setLoading());
        fetching = (async () => {
          try {
            const frame = parseFrame<S, D>(await transport.snapshot(config.channel));
            if (frame?.kind !== 'snapshot') throw new Error('The live feed answered with something that is not a snapshot');
            take(frame, 'fetched');
            patchState(store, setLoaded());
          } catch (e) {
            patchState(store, setError(errorMessage(e, 'The live feed did not answer')));
          } finally {
            fetching = null;
          }
        })();
        return fetching;
      };

      const onPayload = (payload: unknown): void => {
        const frame = parseFrame<S, D>(payload);
        if (frame && take(frame, 'event') === 'gap') void resyncLive();
      };

      return {
        /** Starts listening and loads the first snapshot (idempotent; a second call returns the first). */
        connectLive(): Promise<void> {
          if (connecting) return connecting;
          const mine = generation;
          connecting = (async () => {
            patchState(store, setLoading());
            try {
              const stop = await transport.listen(config.channel, onPayload);
              if (mine !== generation) {
                stop();
                return;
              }
              unlisten = stop;
            } catch (e) {
              connecting = null;
              patchState(store, setError(errorMessage(e, 'The live feed could not be listened to')));
              return;
            }
            await resyncLive();
          })();
          return connecting;
        },
        /** Asks Rust for the whole state now (a window that doubts its state, or Retry after an error). */
        resyncLive,
        /** Stops listening; the last state stays. */
        disconnectLive(): void {
          unlisten?.();
          unlisten = null;
          connecting = null;
          generation += 1;
        },
      };
    }),
    withHooks({
      onDestroy(store) {
        store.disconnectLive();
      },
    }),
  );
}
