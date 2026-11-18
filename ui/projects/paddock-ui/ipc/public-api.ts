/**
 * `@decryp7/paddock-ui/ipc`: the app IPC kit (plan X28). The error every command rejects with (`CommandError`, generated from
 * `paddock_commands::CommandError`) and its checks and conversions; call timeouts for request/reply commands (never for
 * jobs, rule 12); one result per item for bulk commands (`ItemResult`, generated from `paddock_commands::ItemResult`); and the backend selection (Tauri or the lazily imported mock). The mock kit is `@decryp7/paddock-ui/ipc/testing`.
 */
export { ERROR_CODES, type CommandError, type ErrorCode, type ItemResult } from './generated/ipc';
export { isItemOk, parseItemResults, splitItemResults, type SplitItems } from './src/item-result';
export { isCommandError, toCommandError, fromRejection, isServiceOutage, isUnsupported, CommandFailure } from './src/command-error';
export {
  DEFAULT_CALL_TIMEOUT_MS,
  timeoutError,
  withTimeout,
  withCallTimeout,
  callTimeoutProblems,
  limitFor,
  type CallTimeoutPolicy,
  type Timers,
} from './src/call-timeout';
export { backendKind, chooseBackend, inTauri, provideBackend, type BackendChoice, type BackendEnv, type BackendKind } from './src/backend-selection';
