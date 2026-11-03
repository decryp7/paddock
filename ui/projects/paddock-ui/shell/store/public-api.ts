/**
 * `@decryp7/paddock-ui/shell/store`: the shared SignalStore features of the shell and the apps (plan S4): `withRequestStatus`
 * (rule 30: one `idle | loading | loaded | { error }` union per store, never separate flags) and its state helpers.
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
export { injectTrackLoad, type TrackLoad } from './src/track-load';
