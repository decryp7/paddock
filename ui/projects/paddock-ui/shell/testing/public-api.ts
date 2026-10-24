/**
 * `@decryp7/paddock-ui/shell/testing`: the shell's test kit for TestBed (ADR 0012 §7). From plan S2: the contributions
 * lint (`assertContributionsValid`) and the lazy-registration lint (`checkRegistrationSource`). From plan S6: the
 * boot-every-dialog harness (`bootDialog`, `assertDialogBoots`), the static command-contract check (`dialogTableProblems`),
 * `FakeDialogIo` and the IPC recorder. From plan S16: `provideShellTesting` (a zoneless testing window with recorded
 * ports, idle work and a clock run by hand), `settle` and `render` on `whenStable` (rule 38), and the lazy-export lint.
 * The Playwright half of the kit is `@decryp7/paddock-e2e`.
 */
export { contributionProblems, assertContributionsValid } from './src/contributions-lint';
export { checkRegistrationSource, assertRegistrationsLazy, isRegistrationFile, staticImports, dynamicImports, lazyExportProblems } from './src/registration-lint';
export {
  bootDialog,
  assertDialogBoots,
  bootEveryDialog,
  dialogsOf,
  dialogTableProblems,
  type DialogHarnessOptions,
  type DialogBootReport,
  type CommandTable,
} from './src/dialog-harness';
export { FakeDialogIo } from './src/fake-dialog-io';
export { recordIpc, ANY_ANSWER, type IpcRecorder } from './src/ipc-recorder';
export {
  provideShellTesting,
  shellPorts,
  shellIdle,
  settle,
  render,
  RecordingShellPorts,
  ManualIdle,
  ManualClock,
  type ShellTestingOptions,
  type OpenedDialog,
  type Rendered,
} from './src/shell-testing';
