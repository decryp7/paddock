/**
 * `@decryp7/paddock-ui/shell/store`: the shared SignalStore features of the shell and the apps (plan S4): `withRequestStatus`
 * (rule 30: one `idle | loading | loaded | { error }` union per store, never separate flags), its state helpers, and the
 * declared retry of a failed read (ADR 0058: `ReadPolicy`, `READ_RETRY_CLOCK`, the backoff constants).
 */
export {
  withRequestStatus,
  setLoading,
  setLoaded,
  setError,
  errorMessage,
  type RequestStatus,
  type RequestStatusState,
} from './src/request-status';
export {
  READ_RETRY_BACKOFF_MS,
  READ_RETRY_STEADY_MS,
  READ_RETRY_EVENT_SPACING_MS,
  READ_RETRY_CLOCK,
  type ReadRetryClock,
  type ReadPolicy,
  type ReadSpec,
  type Reads,
  type ReadControl,
} from './src/read-retry';
export { injectTrackLoad, type TrackLoad } from './src/track-load';
