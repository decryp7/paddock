import { MemoryChannelsBackend, TauriChannelsBackend, type ChannelConfig, type ChannelsBackend, type TestOutcome } from '@decryp7/paddock-ui/shell/channels';
import { tauriIpc } from './demo-ipc';

const OUTCOMES: readonly TestOutcome[] = ['succeed', 'rejected', 'unreachable', 'missingSecret', 'hang'];

const SLACK: ChannelConfig = {
  id: 'slack',
  name: 'Team Slack',
  enabled: true,
  events: ['diskLow', 'jobFailed', 'serviceDown'],
  minSeverity: 'warning',
  transport: { type: 'webhook', url: 'https://hooks.example.com/services/T0000/B0000/a-rather-long-secret-looking-path-segment-0123456789', auth: true },
};
const PHONE: ChannelConfig = {
  id: 'phone',
  name: 'Phone',
  enabled: true,
  events: ['diskLow'],
  minSeverity: 'error',
  transport: { type: 'ntfy', server: 'https://ntfy.sh', topic: 'demo_alerts', auth: false },
};
const MAIL: ChannelConfig = {
  id: 'mail',
  name: 'Ops mailbox',
  enabled: false,
  events: [],
  minSeverity: 'info',
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

/** How a test ends in the browser build (`?channels-test=succeed|rejected|unreachable|hang`); it is delivered by default. */
export function fakeTestOutcome(search: string = globalThis.location?.search ?? ''): TestOutcome {
  const wanted = new URLSearchParams(search).get('channels-test');
  return OUTCOMES.find((o) => o === wanted) ?? 'succeed';
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
  memory ??= new MemoryChannelsBackend({
    channels: start.channels,
    secrets: start.secrets,
    getFailures: start.getFailures,
    outcome: fakeTestOutcome(),
    retry: fakeTestOutcome() === 'succeed',
    schedule: (step) => void setTimeout(step, 400),
  });
  return memory;
}
