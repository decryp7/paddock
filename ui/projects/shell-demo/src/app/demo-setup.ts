import { MemorySetupBackend, TauriSetupBackend, type SetupBackend, type StepOutcome } from '@decryp7/paddock-ui/shell/setup';
import type { SetupState } from '@decryp7/paddock-ui/shell/setup';
import { tauriIpc } from './demo-ipc';

const OUTCOMES: readonly StepOutcome[] = ['succeed', 'fail', 'hang'];
const SHOWN_KEY = 'sd.setup.shown';

/** How a step ends in the browser build (`?setup-outcome=fail|hang`); it works by default. */
export function setupOutcome(search: string = globalThis.location?.search ?? ''): StepOutcome {
  const wanted = new URLSearchParams(search).get('setup-outcome');
  return OUTCOMES.find((o) => o === wanted) ?? 'succeed';
}

/**
 * Whether the browser build starts as a first run (`?setup=fresh`). Other pages of the demo do not open the Set-up window
 * unasked, so the default is a checklist that was shown already; Settings > Set-up and Help > Set-up… still open it.
 */
export function setupFresh(search: string = globalThis.location?.search ?? ''): boolean {
  return new URLSearchParams(search).get('setup') === 'fresh';
}

function remembered(): boolean {
  try {
    return globalThis.localStorage?.getItem(SHOWN_KEY) === '1';
  } catch {
    return false;
  }
}

function remember(): void {
  try {
    globalThis.localStorage?.setItem(SHOWN_KEY, '1');
  } catch {
    // Storage can be blocked (private windows, previews): the checklist then simply opens again next time.
  }
}

/** The in-memory engine of the browser build, which keeps "shown once" across reloads like the engine's `setup.json`. */
class DemoSetupBackend extends MemorySetupBackend {
  override finish(): Promise<SetupState> {
    remember();
    return super.finish();
  }
}

let memory: SetupBackend | null = null;

/**
 * The checklist over Tauri in the desktop build; in the browser, one in-memory checklist per page (the main window and its
 * in-page dialogs share it, as windows share the real engine). Its steps pace at a visible speed.
 */
export function demoSetupBackend(): SetupBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriSetupBackend(ipc);
  const fresh = setupFresh();
  memory ??= new DemoSetupBackend({
    finished: !fresh || remembered(),
    outcomes: { service: setupOutcome(), login: setupOutcome(), notifications: setupOutcome(), pitwall: setupOutcome() },
    schedule: (step) => void setTimeout(step, 500),
  });
  return memory;
}
