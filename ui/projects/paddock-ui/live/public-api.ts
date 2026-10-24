/**
 * `@decryp7/paddock-ui/live`: the window side of the live feed (plan X29; the Rust side is `paddock-live`). Rust owns the state
 * and pushes a snapshot, then ordered deltas with sequence numbers; the window folds them.
 *
 * - `withLiveFeed`: the SignalStore feature (`live`, `liveSeq`, `liveLink`, `connectLive`, `resyncLive`, `disconnectLive`).
 * - `foldFrame`, `parseFrame`, `initialFolded`: the pure fold (gap, duplicate and stale handling) the feature uses.
 * - `LIVE_TRANSPORT`, `tauriLiveTransport`, `MemoryLiveTransport`: how frames arrive (Tauri events and `live_snapshot`, or memory).
 * - `appendBounded`, `appendThrottled`, `upsertBy`, `prependUnique`: the bounded lists a fold keeps (live tails, event feeds).
 * - `Debouncer`: collects a burst of pushes into one reload.
 */
export { LIVE_COMMANDS, type LiveCommand } from './generated/live-commands';
export {
  foldFrame,
  initialFolded,
  parseFrame,
  parseLink,
  sameLink,
  type FoldOutcome,
  type Folding,
  type FrameSource,
  type LiveFolded,
  type LiveFrame,
  type LiveLink,
} from './src/fold';
export { appendBounded, appendThrottled, prependUnique, upsertBy, type ThrottleOptions } from './src/bounded';
export { Debouncer } from './src/debounce';
export { LIVE_TRANSPORT, MemoryLiveTransport, liveEvent, tauriLiveTransport, type LiveIpc, type LiveTransport, type Unlisten } from './src/live-transport';
export { withLiveFeed, type LiveFeedConfig, type LiveFeedState } from './src/live-feed.store';
