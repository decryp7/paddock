import { MemoryServiceBackend, TauriServiceBackend, type Condition, type FakeOutcome, type ServiceBackend } from '@decryp7/paddock-ui/shell/service';
import { tauriIpc } from './demo-ipc';

const CONDITIONS: readonly Condition[] = ['running', 'notInstalled', 'notRunning', 'refused', 'programMissing', 'moved', 'switchedOff', 'needsApproval', 'unavailable', 'newerOwner'];
const OUTCOMES: readonly FakeOutcome[] = ['succeed', 'fail', 'switchedOff', 'needsApproval', 'hang'];

/** Where the fake service starts in the browser build (`?service=notInstalled|notRunning|switchedOff|...`); not running by default. */
export function fakeCondition(search: string = globalThis.location?.search ?? ''): Condition {
  const wanted = new URLSearchParams(search).get('service');
  return CONDITIONS.find((c) => c === wanted) ?? 'notRunning';
}

/** How a repair or install ends in the browser build (`?service-outcome=fail|hang|...`); it works by default. */
export function fakeOutcome(search: string = globalThis.location?.search ?? ''): FakeOutcome {
  const wanted = new URLSearchParams(search).get('service-outcome');
  return OUTCOMES.find((o) => o === wanted) ?? 'succeed';
}

/**
 * `?service-step=manual`: the service's phases wait until the test calls `window.__sdServiceStep()` (one step per call), so a
 * test sees each phase for as long as it wants instead of racing a 500 ms timer.
 */
function manualSteps(search: string = globalThis.location?.search ?? ''): boolean {
  return new URLSearchParams(search).get('service-step') === 'manual';
}

const queued: (() => void)[] = [];
function queueStep(step: () => void): void {
  queued.push(step);
  (globalThis as { __sdServiceStep?: () => void }).__sdServiceStep = () => queued.shift()?.();
}

let memory: ServiceBackend | null = null;

/**
 * The background service over Tauri in the desktop build; in the browser, one in-memory service per page (the main window and
 * its in-page dialogs share it, as windows share the real engine). Its phases pace at a visible speed.
 */
export function demoServiceBackend(): ServiceBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriServiceBackend(ipc);
  memory ??= new MemoryServiceBackend({
    condition: fakeCondition(),
    outcome: fakeOutcome(),
    schedule: manualSteps() ? queueStep : (step) => void setTimeout(step, 500),
  });
  return memory;
}
