/**
 * `@decryp7/paddock-ui/shell/startup`: the start-up contract (ADR 0058; plan S7 and §11). `provideStartupContract`
 * reports `window_ready` after the first render and answers Rust's liveness pings; `retryUntilAnswered` for start-up
 * reads; boot error forwarding for `main.ts`; the UI checkpoint a recovery reload restores, with its calm notice through
 * the shell's notify port; the backend port with its Tauri and mock implementations; the splash check.
 */
export type { Recovery, ReadyReply, BootErrorKind, PingEvent } from './generated/startup';
export { PING_EVENT } from './generated/startup';
export { STARTUP_COMMANDS, type StartupCommand } from './generated/startup-commands';
export { retryUntilAnswered, STARTUP_RETRY, type RetryOptions } from './src/retry';
export {
  startBootErrorForwarding,
  stopBootErrorForwarding,
  reportBootstrapFailure,
  describeError,
  MAX_BOOT_ERRORS,
  MAX_BOOT_ERROR_LEN,
  type BootErrorSender,
} from './src/boot-errors';
export {
  StartupBackend,
  TauriStartupBackend,
  MockStartupBackend,
  provideStartupBackend,
  ipcBootErrorSender,
  MOCK_RECOVER_KEY,
  MOCK_CHECKPOINT_KEY,
  type StartupIpc,
} from './src/backend';
export { AFTER_READY, WindowReady, windowReady, reportReady, answerPings, provideStartupContract, type ReadySender, type StartupContractOptions } from './src/contract';
export {
  WindowRecovery,
  UI_CHECKPOINT,
  provideUiCheckpoint,
  CHECKPOINT_MS,
  RECOVERY_NOTICES,
  RECOVERY_NOTICE_KIND,
  type UiCheckpoint,
} from './src/recovery';
export { SPLASH_CLASS, splashProblems } from './src/splash';
