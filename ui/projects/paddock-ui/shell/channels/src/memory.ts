import {
  CHANNEL_ID_PATTERN,
  MAX_CHANNELS,
  MAX_EVENTS,
  MAX_RECIPIENTS,
  NTFY_TOPIC_PATTERN,
  type ChannelConfig,
  type ChannelsError,
  type ChannelsEvent,
  type ChannelsSettings,
  type ChannelsView,
} from '../generated/channels';
import { ChannelsBackend, type Unlisten } from './backend';
import { channelNeedsSecret } from './draft';

/** How the fake engine ends a test send. */
export type TestOutcome =
  /** Delivered at the first attempt. */
  | 'succeed'
  /** The receiver answers 503 once: a wait, then delivered at the second attempt. */
  | 'retry'
  /** The receiver refuses it (an HTTP 401 style advice). */
  | 'rejected'
  /** The receiver cannot be reached. */
  | 'unreachable'
  /** The channel needs a secret and none is stored. */
  | 'missingSecret'
  /** It never ends: attempt 1 is sending, for ever (to show "Still working"). */
  | 'hang';

export interface MemoryChannelsOptions {
  /** The channels at the start (default none). */
  readonly channels?: readonly ChannelConfig[];
  /** Channel ids that start with a secret stored. */
  readonly secrets?: readonly string[];
  /** How a test ends (default `succeed`); a function decides per channel. A channel that needs a secret and has none always ends `missingSecret`. */
  readonly outcome?: TestOutcome | ((channel: ChannelConfig) => TestOutcome);
  /** `get` rejects this many times before it answers (to show the inline error and Retry). */
  readonly getFailures?: number;
  /** `save` rejects with this error (to show the engine's message). */
  readonly saveError?: ChannelsError;
  /** Runs the fake engine's send (default: the next microtask). */
  readonly schedule?: (step: () => void) => void;
}

const LOCAL = /^(localhost|127(\.\d{1,3}){3}|\[?::1\]?|.+\.localhost)$/i;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

function urlProblem(label: string, url: string): string | null {
  const host = hostOf(url);
  const scheme = /^(https?):\/\//i.exec(url)?.[1]?.toLowerCase();
  if (!host || !scheme) return `${label} is not a web address.`;
  if (scheme === 'http' && !LOCAL.test(host)) return `${label} must start with https:// (http:// is only allowed for this computer).`;
  return null;
}

/** The first thing the fake engine finds wrong with `channel`, in the engine's words; a small mimic of the real validation. */
function channelProblem(c: ChannelConfig): string | null {
  const name = c.name.trim() || c.id;
  if (!CHANNEL_ID_PATTERN.test(c.id)) return `Channel id ${JSON.stringify(c.id)} may only use letters, digits, dots, hyphens and underscores (1 to 64).`;
  if (!c.name.trim()) return `Channel ${c.id} needs a name.`;
  if (c.events.length > MAX_EVENTS) return `${name} lists more than ${MAX_EVENTS} events.`;
  const t = c.transport;
  switch (t.type) {
    case 'webhook': {
      const p = urlProblem('The address', t.url);
      return p ? `${name}: ${p}` : null;
    }
    case 'ntfy': {
      const p = urlProblem('The server', t.server);
      if (p) return `${name}: ${p}`;
      return NTFY_TOPIC_PATTERN.test(t.topic) ? null : `${name}: the topic may only use letters, digits, hyphens and underscores (1 to 64).`;
    }
    case 'email':
      if (!t.host.trim()) return `${name}: the mail server is missing.`;
      if (!Number.isInteger(t.port) || t.port < 1 || t.port > 65535) return `${name}: the port must be between 1 and 65535.`;
      if (!EMAIL.test(t.from)) return `${name}: the sender is not an email address.`;
      if (t.to.length === 0) return `${name}: add at least one recipient.`;
      if (t.to.length > MAX_RECIPIENTS) return `${name}: at most ${MAX_RECIPIENTS} recipients.`;
      return t.to.every((a) => EMAIL.test(a)) ? null : `${name}: a recipient is not an email address.`;
  }
}

/** The fake engine's validation of the whole settings. */
export function settingsProblem(settings: ChannelsSettings): string | null {
  if (settings.channels.length > MAX_CHANNELS) return `At most ${MAX_CHANNELS} channels.`;
  const ids = new Set<string>();
  for (const c of settings.channels) {
    if (ids.has(c.id)) return `Two channels share the id ${c.id}.`;
    ids.add(c.id);
    const p = channelProblem(c);
    if (p) return p;
  }
  return null;
}

/**
 * The channels held in memory, playing `paddock-channels` (browser build, demos, tests): the same validation messages in
 * spirit, secrets in a map that never leaves it, a test that returns at once and ends by an event. The engine is the
 * reference; this only mimics its contract.
 */
export class MemoryChannelsBackend extends ChannelsBackend {
  private readonly listeners = new Set<(e: ChannelsEvent) => void>();
  private readonly schedule: (step: () => void) => void;
  private readonly secrets = new Map<string, string>();
  private stored: ChannelConfig[];
  private failures: number;
  private nextTest = 1;
  /** The secret of each channel as the fake engine holds it (for tests; the UI never reads secrets). */
  secretOf(id: string): string | undefined {
    return this.secrets.get(id);
  }

  constructor(private readonly options: MemoryChannelsOptions = {}) {
    super();
    this.schedule = options.schedule ?? ((step) => queueMicrotask(step));
    this.stored = [...(options.channels ?? [])];
    this.failures = options.getFailures ?? 0;
    for (const id of options.secrets ?? []) this.secrets.set(id, '(stored)');
  }

  get(): Promise<ChannelsView> {
    if (this.failures > 0) {
      this.failures--;
      return Promise.reject({ code: 'storeUnavailable', message: 'The notification settings could not be read. Try again.', retryable: true } satisfies ChannelsError);
    }
    return Promise.resolve(this.view());
  }

  save(settings: ChannelsSettings): Promise<ChannelsView> {
    if (this.options.saveError) return Promise.reject(this.options.saveError);
    const problem = settingsProblem(settings);
    if (problem) return Promise.reject({ code: 'invalid', message: problem, retryable: false } satisfies ChannelsError);
    this.stored = [...settings.channels];
    // A channel that needs no secret keeps none; a removed channel loses its own.
    for (const id of [...this.secrets.keys()]) {
      const c = this.stored.find((x) => x.id === id);
      if (!c || !channelNeedsSecret(c)) this.secrets.delete(id);
    }
    return Promise.resolve(this.view());
  }

  setSecret(id: string, value: string): Promise<void> {
    if (!value) return Promise.reject({ code: 'invalid', message: 'The secret is empty.', retryable: false } satisfies ChannelsError);
    this.secrets.set(id, value);
    return Promise.resolve();
  }

  clearSecret(id: string): Promise<void> {
    this.secrets.delete(id);
    return Promise.resolve();
  }

  test(channel: ChannelConfig): Promise<number> {
    const problem = channelProblem(channel);
    if (problem) return Promise.reject({ code: 'invalid', message: problem, retryable: false } satisfies ChannelsError);
    const testId = this.nextTest++;
    const wanted = typeof this.options.outcome === 'function' ? this.options.outcome(channel) : (this.options.outcome ?? 'succeed');
    const outcome = channelNeedsSecret(channel) && !this.secrets.has(channel.id) && wanted !== 'hang' ? 'missingSecret' : wanted;
    const done = (ok: boolean, attempts: number, message: string | null): void => this.emit({ kind: 'test', testId, channelId: channel.id, ok, attempts, message });
    const progress = (attempt: number, phase: 'sending' | 'waiting', waitSecs: number | null): void =>
      this.emit({ kind: 'testProgress', testId, channelId: channel.id, attempt, of: 3, phase, waitSecs });
    switch (outcome) {
      case 'hang':
        this.schedule(() => progress(1, 'sending', null));
        break;
      case 'retry':
        this.schedule(() => progress(1, 'sending', null));
        this.schedule(() => progress(1, 'waiting', 4));
        this.schedule(() => progress(2, 'sending', null));
        this.schedule(() => done(true, 2, null));
        break;
      case 'succeed':
        this.schedule(() => done(true, 1, null));
        break;
      case 'rejected':
        this.schedule(() => done(false, 1, 'The receiver refused the message (HTTP 401). Check the token, then send the test again.'));
        break;
      case 'unreachable':
        this.schedule(() => done(false, 3, 'The receiver could not be reached after 3 attempts. Check the address and your connection.'));
        break;
      case 'missingSecret':
        this.schedule(() => done(false, 0, 'This channel needs a token and none is stored yet. Set the token first.'));
        break;
    }
    return Promise.resolve(testId);
  }

  listen(handler: (event: ChannelsEvent) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }

  /** How many listeners are attached (for tests of the unlisten lifecycle). */
  get listenerCount(): number {
    return this.listeners.size;
  }

  /** Sends `event` to the listeners, as the engine does (also used by tests for a late or foreign event). */
  emit(event: ChannelsEvent): void {
    for (const l of [...this.listeners]) l(event);
  }

  private view(): ChannelsView {
    const secretSet: Record<string, boolean> = {};
    for (const c of this.stored) if (channelNeedsSecret(c) && this.secrets.has(c.id)) secretSet[c.id] = true;
    return { settings: { channels: [...this.stored] }, secretSet };
  }
}
