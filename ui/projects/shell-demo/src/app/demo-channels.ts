import { MemoryChannelsBackend, TauriChannelsBackend, type ChannelConfig, type ChannelsBackend, type TestOutcome } from '@decryp7/paddock-ui/shell/channels';
import { tauriIpc } from './demo-ipc';

const OUTCOMES: readonly TestOutcome[] = ['succeed', 'retry', 'rejected', 'unreachable', 'missingSecret', 'hang'];

const SLACK: ChannelConfig = {
  id: 'slack',
  name: 'Team Slack',
  enabled: true,
  events: ['diskLow', 'jobFailed', 'oldAlarm'],
  minSeverity: 'warning',
  allowLan: false,
  transport: { type: 'webhook', url: 'https://hooks.example.com/services/T0000/B0000/a-rather-long-secret-looking-path-segment-0123456789', auth: true },
};
const PHONE: ChannelConfig = {
  id: 'phone',
  name: 'Phone',
  enabled: true,
  events: ['diskLow'],
  minSeverity: 'error',
  allowLan: false,
  transport: { type: 'ntfy', server: 'https://ntfy.sh', topic: 'demo_alerts', auth: false },
};
const MAIL: ChannelConfig = {
  id: 'mail',
  name: 'Ops mailbox',
  enabled: false,
  events: [],
  minSeverity: 'info',
  allowLan: false,
  transport: { type: 'email', host: 'smtp.example.com', port: 587, security: 'startTls', username: 'ops', from: 'demo@example.com', to: ['ops@example.com'] },
};

/** The channels the browser build starts with (`?channels=empty|many|error`; two by default). */
export function fakeChannels(search: string = globalThis.location?.search ?? ''): { channels: ChannelConfig[]; getFailures: number; secrets: string[] } {
  const wanted = new URLSearchParams(search).get('channels');
  if (wanted === 'empty') return { channels: [], getFailures: 0, secrets: [] };
  if (wanted === 'error') return { channels: [], getFailures: 1, secrets: [] };
  if (wanted === 'many') {
    const extra = Array.from({ length: 5 }, (_, i): ChannelConfig => ({ ...PHONE, id: `topic-${i + 1}`, name: `Topic ${i + 1}`, transport: { ...PHONE.transport, topic: `demo_${i + 1}` } as ChannelConfig['transport'] }));
    return { channels: [SLACK, PHONE, MAIL, ...extra], getFailures: 0, secrets: ['slack'] };
  }
  return { channels: [SLACK, PHONE], getFailures: 0, secrets: ['slack'] };
}

/** How a test ends in the browser build (`?channels-test=succeed|retry|rejected|unreachable|hang`); it is delivered by default. */
export function fakeTestOutcome(search: string = globalThis.location?.search ?? ''): TestOutcome {
  const wanted = new URLSearchParams(search).get('channels-test');
  return OUTCOMES.find((o) => o === wanted) ?? 'succeed';
}

/**
 * `?channels-step=manual`: the engine's steps wait until the test calls `window.__sdChannelsStep()` (one step per call), so a
 * test sees each phase of a test send for as long as it wants instead of racing a 400 ms timer.
 */
function manualSteps(search: string = globalThis.location?.search ?? ''): boolean {
  return new URLSearchParams(search).get('channels-step') === 'manual';
}

const queued: (() => void)[] = [];
function queueStep(step: () => void): void {
  queued.push(step);
  (globalThis as { __sdChannelsStep?: () => void }).__sdChannelsStep = () => queued.shift()?.();
}

let memory: ChannelsBackend | null = null;

/**
 * The channels over Tauri in the desktop build; in the browser, one in-memory engine per page (the Settings window and the
 * secret dialog share it, as windows share the real engine). A test takes a visible moment.
 */
export function demoChannelsBackend(): ChannelsBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriChannelsBackend(ipc);
  const start = fakeChannels();
  if (!memory) {
    const engine = new MemoryChannelsBackend({
      channels: start.channels,
      secrets: start.secrets,
      getFailures: start.getFailures,
      outcome: fakeTestOutcome(),
      schedule: manualSteps() ? queueStep : (step) => void setTimeout(step, 400),
    });
    // The secret dialog's busy and rejected states (`?channels-secret=slow|refuse`): storing never answers, or is refused.
    const secret = new URLSearchParams(globalThis.location?.search ?? '').get('channels-secret');
    if (secret === 'slow') engine.setSecret = () => new Promise(() => undefined);
    if (secret === 'refuse') engine.setSecret = () => Promise.reject({ code: 'storeUnavailable', message: 'The keychain is locked. Unlock it and try again.', retryable: true });
    memory = engine;
  }
  return memory;
}
