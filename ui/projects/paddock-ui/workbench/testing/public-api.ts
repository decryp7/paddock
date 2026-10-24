/**
 * `@decryp7/paddock-ui/workbench/testing`: the workbench's test kit for TestBed (ADR 0013 §4; plan W7):
 * `provideWorkbenchTesting` (the shell's testing window plus recorded `WORKBENCH_PORTS` and the workbench's kinds) and
 * `RecordingWorkbenchPorts`. The Playwright half is `@decryp7/paddock-e2e` (`WorkbenchApp` and its page objects).
 */
export { provideWorkbenchTesting, RecordingWorkbenchPorts, type WorkbenchTestingOptions } from './src/workbench-testing';
