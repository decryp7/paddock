/**
 * `@decryp7/paddock-ui/shell/testing`: the shell's test kit for TestBed (ADR 0012 §7). From plan S2: the contributions
 * lint (`assertContributionsValid`) and the lazy-registration lint (`checkRegistrationSource`). From plan S6: the
 * boot-every-dialog harness (`bootDialog`, `assertDialogBoots`), the static command-contract check (`dialogTableProblems`),
 * `FakeDialogIo` and the IPC recorder. `provideShellTesting` arrives with plan S16.
 */
export { contributionProblems, assertContributionsValid } from './src/contributions-lint';
export { checkRegistrationSource, assertRegistrationsLazy, isRegistrationFile, staticImports } from './src/registration-lint';
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
