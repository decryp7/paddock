import { Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideContributions } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { CHANNELS_EVENT, MAX_CHANNELS, type ChannelConfig, type ChannelsEvent } from '../generated/channels';
import { CHANNELS_COMMANDS, type ChannelsCommand } from '../generated/channels-commands';
import { ChannelsBackend, TauriChannelsBackend, type ChannelsIpc } from './backend';
import { ChannelsStore } from './channels.store';
import {
  CHANNELS_SECRET_DIALOG_COMMANDS,
  CHANNELS_SECRET_DIALOG_ID,
  CHANNELS_SETTINGS_COMMANDS,
  CHANNELS_SETTINGS_PAGE,
  CHANNELS_REMOVE_DIALOG_ID,
  channelsContributions,
  defineChannelsRemoveDialog,
  defineChannelsSecretDialog,
  defineChannelsSettingsPage,
} from './channels.contributions';
import { CHANNEL_EVENTS, groupEvents, toggleEvent, unknownEvents } from './events';
import { EMPTY_DRAFT, channelNeedsSecret, draftFromChannel, draftHints, draftToChannel, newChannelId, secretKind, tokens } from './draft';
import { MemoryChannelsBackend, settingsProblem, type MemoryChannelsOptions } from './memory';
import { provideChannelsBackend, provideShellChannels } from './provide-channels';
import { asChannelsError, channelsMessage, parseChannel, parseChannelsEvent, parseChannelsView, parseTestStarted } from './wire';

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

export const WEBHOOK: ChannelConfig = {
  id: 'slack',
  name: 'Slack',
  enabled: true,
  events: ['diskLow', 'jobFailed'],
  minSeverity: 'warning',
  allowLan: false,
  transport: { type: 'webhook', url: 'https://hooks.example.com/services/abc', auth: true },
};
export const NTFY: ChannelConfig = {
  id: 'phone',
  name: 'Phone',
  enabled: false,
  events: [],
  minSeverity: 'error',
  allowLan: false,
  transport: { type: 'ntfy', server: 'https://ntfy.sh', topic: 'my_topic', auth: false },
};
export const EMAIL: ChannelConfig = {
  id: 'mail',
  name: 'Mail',
  enabled: true,
  events: ['diskLow'],
  minSeverity: 'info',
  allowLan: false,
  transport: { type: 'email', host: 'smtp.example.com', port: 587, security: 'startTls', username: 'me', from: 'app@example.com', to: ['me@example.com'] },
};

/** The engine's send waits until `next()`/`all()`: a test sees each phase. */
function held(options: MemoryChannelsOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryChannelsBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    async all(): Promise<void> {
      while (queue.length) queue.shift()?.();
      await settle();
    },
    pending: () => queue.length,
  };
}

function storeBed(backend: ChannelsBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellChannels({ backend: () => backend })] });
  return TestBed.inject(ChannelsStore);
}

describe('the wire', () => {
  it('rebuilds every transport from known fields and drops what is not a channel', () => {
    for (const c of [WEBHOOK, NTFY, EMAIL]) expect(parseChannel({ ...c, extra: 1 })).toEqual(c);
    expect(parseChannel({ ...EMAIL, transport: { ...EMAIL.transport, username: null } })).toMatchObject({ transport: { username: null } });
    const bad: unknown[] = [
      null,
      { ...WEBHOOK, id: 3 },
      { ...WEBHOOK, minSeverity: 'fatal' },
      { ...WEBHOOK, events: ['a', 1] },
      { ...WEBHOOK, transport: { type: 'sms' } },
      { ...WEBHOOK, transport: { type: 'webhook', url: 'x' } },
      { ...NTFY, transport: { type: 'ntfy', server: 's', topic: 1, auth: false } },
      { ...EMAIL, transport: { ...EMAIL.transport, port: 'x' } },
      { ...EMAIL, transport: { ...EMAIL.transport, port: 1.5 } },
      { ...EMAIL, transport: { ...EMAIL.transport, security: 'ssl' } },
      { ...EMAIL, transport: { ...EMAIL.transport, to: 'a@b.c' } },
      { ...EMAIL, transport: { ...EMAIL.transport, username: 4 } },
    ];
    for (const b of bad) expect(parseChannel(b)).toBeNull();
  });

  it('parses a view with its secret flags, and refuses a malformed one', () => {
    expect(parseChannelsView({ settings: { channels: [WEBHOOK] }, secretSet: { slack: true, other: false, odd: 'yes' } })).toEqual({ settings: { channels: [WEBHOOK] }, secretSet: { slack: true } });
    expect(parseChannelsView({ settings: { channels: [] } })).toEqual({ settings: { channels: [] }, secretSet: {} });
    for (const b of [null, {}, { settings: {} }, { settings: { channels: [{}] } }, { settings: { channels: [] }, secretSet: 3 }]) expect(parseChannelsView(b)).toBeNull();
  });

  it('parses events, test ids and the engine’s refusals', () => {
    const ev = { kind: 'test', testId: 4, channelId: 'slack', ok: false, attempts: 2, message: 'No.' };
    expect(parseChannelsEvent({ ...ev, extra: 1 })).toEqual(ev);
    expect(parseChannelsEvent({ ...ev, message: undefined })).toMatchObject({ message: null });
    for (const b of [null, { ...ev, kind: 'other' }, { ...ev, testId: 'x' }, { ...ev, channelId: 1 }, { ...ev, ok: 'y' }, { ...ev, attempts: -1 }, { ...ev, attempts: 1.5 }, { ...ev, message: 4 }]) {
      expect(parseChannelsEvent(b)).toBeNull();
    }
    const progress = { kind: 'testProgress', testId: 4, channelId: 'slack', attempt: 2, of: 3, phase: 'waiting', waitSecs: 4 };
    expect(parseChannelsEvent({ ...progress, extra: 1 })).toEqual(progress);
    expect(parseChannelsEvent({ ...progress, phase: 'sending', waitSecs: undefined })).toMatchObject({ phase: 'sending', waitSecs: null });
    for (const b of [{ ...progress, phase: 'idle' }, { ...progress, attempt: 0 }, { ...progress, attempt: 4 }, { ...progress, of: 'x' }, { ...progress, waitSecs: -1 }, { ...progress, waitSecs: 'x' }, { ...progress, testId: 'x' }]) {
      expect(parseChannelsEvent(b)).toBeNull();
    }
    expect(parseTestStarted({ testId: 7 })).toBe(7);
    expect(parseTestStarted({ testId: 'x' })).toBeNull();
    expect(asChannelsError({ code: 'invalid', message: 'No.' })).toEqual({ code: 'invalid', message: 'No.', retryable: false });
    expect(asChannelsError({ code: 'weird', message: 'No.' })).toBeNull();
    expect(channelsMessage({ code: 'busy', message: 'Wait.', retryable: true }, 'f')).toBe('Wait.');
    expect(channelsMessage(new Error('boom'), 'f')).toBe('boom');
    expect(channelsMessage('plain', 'f')).toBe('plain');
    expect(channelsMessage(7, 'f')).toBe('f');
  });
});

describe('the draft', () => {
  it('round-trips every transport and makes distinct ids', () => {
    for (const c of [WEBHOOK, NTFY, EMAIL]) expect(draftToChannel(draftFromChannel(c), [])).toEqual(c);
    expect(newChannelId('My Slack #1', [])).toBe('My-Slack-1');
    expect(newChannelId('My Slack #1', ['My-Slack-1', 'My-Slack-1-2'])).toBe('My-Slack-1-3');
    expect(newChannelId('???', [])).toBe('channel');
    expect(draftToChannel({ ...EMPTY_DRAFT, name: ' Slack ', url: ' https://x.example ' }, ['Slack'])).toMatchObject({ id: 'Slack-2', name: 'Slack', transport: { url: 'https://x.example' } });
    expect(draftToChannel({ ...draftFromChannel(EMAIL), username: '  ' }, [])).toMatchObject({ transport: { username: null } });
    expect(tokens('a, b;c\nb  d')).toEqual(['a', 'b', 'c', 'd']);
  });

  it('knows which channels need a secret and what it is called', () => {
    expect([WEBHOOK, NTFY, EMAIL].map(channelNeedsSecret)).toEqual([true, false, true]);
    expect(channelNeedsSecret({ ...EMAIL, transport: { ...EMAIL.transport, username: null } as never })).toBe(false);
    expect([WEBHOOK, EMAIL].map(secretKind)).toEqual(['token', 'password']);
  });

  it('gives hints for what is missing and none for a complete draft', () => {
    expect(Object.keys(draftHints(EMPTY_DRAFT)).sort()).toEqual(['name', 'url']);
    expect(Object.keys(draftHints({ ...EMPTY_DRAFT, type: 'ntfy', name: 'n', server: '' })).sort()).toEqual(['server', 'topic']);
    expect(Object.keys(draftHints({ ...EMPTY_DRAFT, type: 'email', name: 'n', port: '70000', to: 'nobody' })).sort()).toEqual(['from', 'host', 'port', 'to']);
    expect(Object.keys(draftHints({ ...EMPTY_DRAFT, type: 'email', name: 'n', port: 'x' }))).toContain('port');
    for (const c of [WEBHOOK, NTFY, EMAIL]) expect(draftHints(draftFromChannel(c))).toEqual({});
  });
});

describe('the in-memory engine', () => {
  it('validates like the engine and keeps secrets in a map', async () => {
    const m = new MemoryChannelsBackend();
    const rejection = async (c: ChannelConfig[]) => (await m.save({ channels: c }).catch((e: unknown) => e)) as { code: string; message: string };
    expect((await rejection([{ ...WEBHOOK, id: 'bad id' }])).code).toBe('invalid');
    expect((await rejection([{ ...WEBHOOK, name: ' ' }])).message).toContain('needs a name');
    expect((await rejection([{ ...WEBHOOK, transport: { type: 'webhook', url: 'http://example.com/x', auth: false } }])).message).toContain('https://');
    expect((await rejection([{ ...WEBHOOK, transport: { type: 'webhook', url: 'ftp://x', auth: false } }])).message).toContain('not a web address');
    expect((await rejection([{ ...NTFY, transport: { type: 'ntfy', server: 'https://ntfy.sh', topic: 'a b', auth: false } }])).message).toContain('topic');
    expect((await rejection([{ ...NTFY, transport: { type: 'ntfy', server: 'nope', topic: 'a', auth: false } }])).message).toContain('server');
    expect((await rejection([WEBHOOK, WEBHOOK])).message).toContain('share the id');
    expect((await rejection([{ ...WEBHOOK, events: Array.from({ length: 65 }, (_, i) => `e${i}`) }])).message).toContain('64');
    const many = Array.from({ length: MAX_CHANNELS + 1 }, (_, i) => ({ ...NTFY, id: `c${i}` }));
    expect((await rejection(many)).message).toContain(String(MAX_CHANNELS));
    const email = EMAIL.transport as Extract<ChannelConfig['transport'], { type: 'email' }>;
    for (const [patch, word] of [
      [{ host: ' ' }, 'mail server'],
      [{ port: 0 }, 'port'],
      [{ from: 'x' }, 'sender'],
      [{ to: [] }, 'recipient'],
      [{ to: Array.from({ length: 11 }, (_, i) => `a${i}@b.co`) }, 'at most'],
      [{ to: ['nobody'] }, 'recipient'],
    ] as const) {
      expect((await rejection([{ ...EMAIL, transport: { ...email, ...patch } as ChannelConfig['transport'] }])).message).toContain(word);
    }
    expect(settingsProblem({ channels: [EMAIL, WEBHOOK, NTFY, { ...NTFY, id: 'local', transport: { type: 'webhook', url: 'http://localhost:8080/x', auth: false } }] })).toBeNull();

    await m.save({ channels: [WEBHOOK] });
    await m.setSecret('slack', 'tok');
    expect(m.secretOf('slack')).toBe('tok');
    expect((await m.get()).secretSet).toEqual({ slack: true });
    await expect(m.setSecret('slack', '')).rejects.toMatchObject({ code: 'invalid' });
    // A channel that no longer needs a secret loses it on save.
    await m.save({ channels: [{ ...WEBHOOK, transport: { type: 'webhook', url: WEBHOOK.transport.type === 'webhook' ? WEBHOOK.transport.url : '', auth: false } }] });
    expect(m.secretOf('slack')).toBeUndefined();
    await m.clearSecret('slack');
  });

  it('plays each test outcome, with a missing secret and a rejected channel', async () => {
    const run = async (outcome: MemoryChannelsOptions['outcome'], channel = WEBHOOK, secrets: string[] = ['slack']): Promise<Extract<ChannelsEvent, { kind: 'test' }>> => {
      const m = new MemoryChannelsBackend({ outcome, secrets });
      const seen: ChannelsEvent[] = [];
      await m.listen((e) => seen.push(e));
      const id = await m.test(channel);
      await settle();
      const finals = seen.filter((e): e is Extract<ChannelsEvent, { kind: 'test' }> => e.kind === 'test');
      expect(finals).toHaveLength(1);
      expect(finals[0]?.testId).toBe(id);
      return finals[0]!;
    };
    expect(await run('succeed')).toMatchObject({ ok: true, attempts: 1, message: null });
    expect(await run('retry')).toMatchObject({ ok: true, attempts: 2 });
    expect((await run('rejected')).message).toContain('401');
    expect((await run('unreachable')).attempts).toBe(3);
    expect((await run('missingSecret')).message).toContain('none is stored');
    expect(await run(undefined, WEBHOOK, [])).toMatchObject({ ok: false, attempts: 0 });
    expect(await run((c) => (c.id === 'phone' ? 'rejected' : 'succeed'), NTFY, [])).toMatchObject({ ok: false });
    const m = new MemoryChannelsBackend({ outcome: 'hang', schedule: () => void 0 });
    await expect(m.test({ ...WEBHOOK, name: '' })).rejects.toMatchObject({ code: 'invalid' });
    expect(await m.test(WEBHOOK)).toBe(1);
    expect(await m.test(WEBHOOK)).toBe(2);
  });

  it('plays the progress of a retry and of a hang as phase-labelled events', async () => {
    const seen: ChannelsEvent[] = [];
    const retry = new MemoryChannelsBackend({ outcome: 'retry', secrets: ['slack'] });
    await retry.listen((e) => seen.push(e));
    await retry.test(WEBHOOK);
    await settle();
    expect(seen.map((e) => (e.kind === 'testProgress' ? `${e.attempt}/${e.of} ${e.phase} ${e.waitSecs}` : 'done'))).toEqual(['1/3 sending null', '1/3 waiting 4', '2/3 sending null', 'done']);
    seen.length = 0;
    const hang = new MemoryChannelsBackend({ outcome: 'hang', secrets: ['slack'] });
    await hang.listen((e) => seen.push(e));
    await hang.test(WEBHOOK);
    await settle();
    expect(seen).toMatchObject([{ kind: 'testProgress', attempt: 1, phase: 'sending' }]);
  });

  it('fails a get on request and refuses a save with a given error', async () => {
    const m = new MemoryChannelsBackend({ getFailures: 1, saveError: { code: 'storeUnavailable', message: 'Disk full.', retryable: true } });
    await expect(m.get()).rejects.toMatchObject({ code: 'storeUnavailable' });
    expect((await m.get()).settings.channels).toEqual([]);
    await expect(m.save({ channels: [] })).rejects.toMatchObject({ message: 'Disk full.' });
  });
});

describe('the Tauri backend', () => {
  function ipc(answers: Partial<Record<ChannelsCommand, unknown>>) {
    const calls: { command: ChannelsCommand; args?: Record<string, unknown> }[] = [];
    let handler: ((p: unknown) => void) | undefined;
    const port: ChannelsIpc = {
      invoke: <T>(command: ChannelsCommand, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        const a = answers[command];
        return a instanceof Error ? Promise.reject(a) : Promise.resolve(a as T);
      },
      listen: (event, h) => {
        expect(event).toBe(CHANNELS_EVENT);
        handler = h;
        return Promise.resolve(() => (handler = undefined));
      },
    };
    return { port, calls, emit: (p: unknown) => handler?.(p), listening: () => handler !== undefined };
  }

  it('calls the commands of the table with the argument names the adapter expects', async () => {
    const view = { settings: { channels: [WEBHOOK] }, secretSet: { slack: true } };
    const t = ipc({ channels_get: view, channels_save: view, channels_set_secret: null, channels_clear_secret: null, channels_test: { testId: 9 } });
    const b = new TauriChannelsBackend(t.port);
    expect(await b.get()).toEqual(view);
    expect(await b.save({ channels: [WEBHOOK] })).toEqual(view);
    await b.setSecret('slack', 'tok');
    await b.clearSecret('slack');
    expect(await b.test(WEBHOOK)).toBe(9);
    expect(t.calls).toEqual([
      { command: 'channels_get', args: undefined },
      { command: 'channels_save', args: { settings: { channels: [WEBHOOK] } } },
      { command: 'channels_set_secret', args: { id: 'slack', value: 'tok' } },
      { command: 'channels_clear_secret', args: { id: 'slack' } },
      { command: 'channels_test', args: { channel: WEBHOOK } },
    ]);
    for (const c of t.calls) expect(CHANNELS_COMMANDS[c.command]).toBeDefined();
  });

  it('refuses malformed payloads, lets the engine’s refusal through, and passes parsed events only', async () => {
    const t = ipc({ channels_get: { nonsense: true }, channels_save: [], channels_test: { testId: 'x' } });
    const b = new TauriChannelsBackend(t.port);
    await expect(b.get()).rejects.toThrow('no valid notification channels');
    await expect(b.save({ channels: [] })).rejects.toThrow('no valid notification channels');
    await expect(b.test(WEBHOOK)).rejects.toThrow('did not say which test');
    const refusal = { code: 'invalid', message: 'No.', retryable: false };
    const r = new TauriChannelsBackend(ipc({ channels_save: refusal as never }).port);
    // A rejected invoke carries the engine's object; here the answer is simply not a view.
    await expect(r.save({ channels: [] })).rejects.toThrow();

    const seen: ChannelsEvent[] = [];
    const off = await b.listen((e) => seen.push(e));
    t.emit({ nonsense: 1 });
    t.emit({ kind: 'test', testId: 1, channelId: 'a', ok: true, attempts: 1, message: null });
    expect(seen).toHaveLength(1);
    off();
    expect(t.listening()).toBe(false);
  });
});

describe('the store', () => {
  const view = (...channels: ChannelConfig[]) => ({ channels });

  it('loads, saves, validates through the engine and removes with its secret', async () => {
    const memory = new MemoryChannelsBackend({ channels: [WEBHOOK], secrets: ['slack'] });
    const store = storeBed(memory);
    expect(store.isLoaded()).toBe(false);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.channels()).toEqual([WEBHOOK]);
    expect(store.secretSet()).toEqual({ slack: true });
    expect(store.atLimit()).toBe(false);

    expect(await store.upsert(NTFY)).toBe(true);
    expect(store.channels().map((c) => c.id)).toEqual(['slack', 'phone']);
    expect(await store.upsert({ ...NTFY, name: 'Renamed' })).toBe(true);
    expect(store.channels().map((c) => c.name)).toEqual(['Slack', 'Renamed']);
    expect(store.saveError()).toBeNull();

    // The engine's message is kept verbatim.
    expect(await store.upsert({ ...NTFY, id: 'bad id' })).toBe(false);
    expect(store.saveError()).toContain('may only use letters');
    expect(store.channels()).toHaveLength(2);
    store.dismissSaveError();
    expect(store.saveError()).toBeNull();

    expect(await store.remove('slack')).toBe(true);
    expect(store.channels().map((c) => c.id)).toEqual(['phone']);
    expect(memory.secretOf('slack')).toBeUndefined();
    expect(store.secretSet()).toEqual({});
  });

  it('keeps the list when a removal is refused, and knows when it is full', async () => {
    const full = Array.from({ length: MAX_CHANNELS }, (_, i) => ({ ...NTFY, id: `c${i}` }));
    const store = storeBed(new MemoryChannelsBackend({ channels: full, saveError: { code: 'busy', message: 'Try later.', retryable: true } }));
    await settle();
    expect(store.atLimit()).toBe(true);
    expect(await store.remove('c0')).toBe(false);
    expect(store.channels()).toHaveLength(MAX_CHANNELS);
    expect(store.saveError()).toBe('Try later.');
  });

  it('shows a failed first load as an error and Retry loads it', async () => {
    const store = storeBed(new MemoryChannelsBackend({ channels: [NTFY], getFailures: 1 }));
    await settle();
    expect(store.requestError()).toContain('could not be read');
    await store.load();
    expect(store.isLoaded()).toBe(true);
    expect(store.channels()).toEqual(view(NTFY).channels);
  });

  it('removes a secret and warns when the secret of a removed channel cannot be deleted', async () => {
    class Stubborn extends MemoryChannelsBackend {
      override clearSecret(): Promise<void> {
        return Promise.reject(new Error('keychain locked'));
      }
    }
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const store = storeBed(new Stubborn({ channels: [WEBHOOK], secrets: ['slack'] }));
    await settle();
    expect(await store.clearSecret('slack')).toBe(false);
    expect(store.saveError()).toBe('keychain locked');
    expect(await store.remove('slack')).toBe(true);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('keychain locked'));
    warn.mockRestore();

  });

  it('removes the stored secret of a channel and refreshes the flags', async () => {
    const again = storeBed(new MemoryChannelsBackend({ channels: [WEBHOOK], secrets: ['slack'] }));
    await settle();
    expect(await again.clearSecret('slack')).toBe(true);
    expect(again.secretSet()).toEqual({});
  });

  it('runs a test: running at once, then the outcome from the event', async () => {
    const h = held({ channels: [WEBHOOK], secrets: ['slack'] });
    const store = storeBed(h.memory);
    await settle();
    const started = store.test(WEBHOOK);
    expect(store.tests()['slack']).toEqual({ state: 'running', testId: null, progress: null });
    await started;
    expect(store.tests()['slack']).toEqual({ state: 'running', testId: 1, progress: null });
    // A second press while it runs does nothing.
    await store.test(WEBHOOK);
    expect(h.pending()).toBe(1);
    await h.all();
    expect(store.tests()['slack']).toEqual({ state: 'delivered', attempts: 1 });
    store.clearTest('slack');
    expect(store.tests()['slack']).toBeUndefined();
  });

  it('shows the engine’s advice when a test fails or is refused, and ignores a stale or foreign outcome', async () => {
    const h = held({ channels: [WEBHOOK], outcome: 'rejected', secrets: ['slack'] });
    const store = storeBed(h.memory);
    await settle();
    await store.test(WEBHOOK);
    // An outcome for another test id and one for another channel change nothing.
    h.memory.emit({ kind: 'test', testId: 99, channelId: 'slack', ok: true, attempts: 1, message: null });
    h.memory.emit({ kind: 'test', testId: 1, channelId: 'other', ok: true, attempts: 1, message: null });
    expect(store.tests()['slack']?.state).toBe('running');
    await h.all();
    expect(store.tests()['slack']).toMatchObject({ state: 'failed', attempts: 1, message: expect.stringContaining('401') });
    // A late event after the outcome changes nothing.
    h.memory.emit({ kind: 'test', testId: 1, channelId: 'slack', ok: true, attempts: 1, message: null });
    expect(store.tests()['slack']?.state).toBe('failed');
    store.clearTest('slack');

    // A refusal to start (the engine's message) is the test's result; a message-less failure gets plain words.
    await store.test({ ...WEBHOOK, name: '' });
    expect(store.tests()['slack']).toMatchObject({ state: 'failed', message: expect.stringContaining('needs a name') });
    store.clearTest('slack');
    await store.test(WEBHOOK);
    h.memory.emit({ kind: 'test', testId: 2, channelId: 'slack', ok: false, attempts: 3, message: null });
    expect(store.tests()['slack']).toMatchObject({ state: 'failed', message: expect.stringContaining('did not get through') });
  });

  it('accepts an outcome that arrives before the answer', async () => {
    class Eager extends MemoryChannelsBackend {
      override async test(c: ChannelConfig): Promise<number> {
        const id = await super.test(c);
        this.emit({ kind: 'test', testId: id, channelId: c.id, ok: true, attempts: 1, message: null });
        return id;
      }
    }
    const store = storeBed(new Eager({ channels: [WEBHOOK], secrets: ['slack'], schedule: () => undefined }));
    await settle();
    const started = store.test(WEBHOOK);
    // The event lands while the answer is still on its way: the first event matches a test without an id.
    await started;
    expect(store.tests()['slack']).toEqual({ state: 'delivered', attempts: 1 });
  });

  it('shows the phase of a running test and never lets the attempt go backwards', async () => {
    const h = held({ channels: [WEBHOOK], secrets: ['slack'], outcome: 'retry' });
    const store = storeBed(h.memory);
    await settle();
    await store.test(WEBHOOK);
    const progress = () => (store.tests()['slack'] as { progress: unknown }).progress;
    h.memory.emit({ kind: 'testProgress', testId: 1, channelId: 'slack', attempt: 2, of: 3, phase: 'waiting', waitSecs: 4 });
    expect(progress()).toEqual({ attempt: 2, of: 3, phase: 'waiting', waitSecs: 4 });
    // A late event of attempt 1 changes nothing; the same attempt in another phase does.
    h.memory.emit({ kind: 'testProgress', testId: 1, channelId: 'slack', attempt: 1, of: 3, phase: 'sending', waitSecs: null });
    expect(progress()).toMatchObject({ attempt: 2, phase: 'waiting' });
    h.memory.emit({ kind: 'testProgress', testId: 1, channelId: 'slack', attempt: 2, of: 3, phase: 'sending', waitSecs: null });
    expect(progress()).toMatchObject({ attempt: 2, phase: 'sending' });
    // Progress of another test or channel is ignored.
    h.memory.emit({ kind: 'testProgress', testId: 9, channelId: 'slack', attempt: 3, of: 3, phase: 'sending', waitSecs: null });
    h.memory.emit({ kind: 'testProgress', testId: 1, channelId: 'other', attempt: 3, of: 3, phase: 'sending', waitSecs: null });
    expect(progress()).toMatchObject({ attempt: 2 });
    await h.all();
    expect(store.tests()['slack']).toEqual({ state: 'delivered', attempts: 2 });
    // Progress after the outcome changes nothing.
    h.memory.emit({ kind: 'testProgress', testId: 1, channelId: 'slack', attempt: 3, of: 3, phase: 'sending', waitSecs: null });
    expect(store.tests()['slack']?.state).toBe('delivered');
  });

  it('never times out a test that hangs', async () => {
    vi.useFakeTimers();
    try {
      const h = held({ channels: [WEBHOOK], secrets: ['slack'], outcome: 'hang' });
      const store = storeBed(h.memory);
      await settle();
      await store.test(WEBHOOK);
      await vi.advanceTimersByTimeAsync(10 * 60_000);
      await h.all();
      expect(store.tests()['slack']).toMatchObject({ state: 'running', testId: 1, progress: { attempt: 1, phase: 'sending' } });
      expect(h.pending()).toBe(0);
      await vi.advanceTimersByTimeAsync(10 * 60_000);
      expect(store.tests()['slack']?.state).toBe('running');
    } finally {
      vi.useRealTimers();
    }
  });

  it('stops listening when destroyed, also when the listen was still pending, and warns when listening fails', async () => {
    let off = 0;
    class Listening extends MemoryChannelsBackend {
      override listen(): Promise<() => void> {
        return Promise.resolve(() => void off++);
      }
    }
    expect(storeBed(new Listening())).toBeTruthy();
    await settle();
    TestBed.resetTestingModule();
    expect(off).toBe(1);

    expect(storeBed(new Listening())).toBeTruthy();
    TestBed.resetTestingModule();
    await settle();
    expect(off).toBe(2);

    // After the store is gone a late event reaches nobody.
    const memory = new MemoryChannelsBackend({ channels: [WEBHOOK], secrets: ['slack'] });
    const store = storeBed(memory);
    await settle();
    expect(memory.listenerCount).toBe(1);
    await store.test(WEBHOOK);
    TestBed.resetTestingModule();
    expect(memory.listenerCount).toBe(0);

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    class Deaf extends MemoryChannelsBackend {
      override listen(): Promise<() => void> {
        return Promise.reject(new Error('no events'));
      }
    }
    const deaf = storeBed(new Deaf());
    await settle();
    expect(deaf.isLoaded()).toBe(true);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });

  it('keeps the list on screen when a quiet refresh fails', async () => {
    class Flaky extends MemoryChannelsBackend {
      fail = false;
      override get() {
        return this.fail ? Promise.reject(new Error('gone')) : super.get();
      }
    }
    const b = new Flaky({ channels: [NTFY] });
    const store = storeBed(b);
    await settle();
    b.fail = true;
    await store.refresh();
    expect(store.channels()).toEqual([NTFY]);
    expect(store.isLoaded()).toBe(true);
  });
});

describe('the contributions', () => {
  const page = defineChannelsSettingsPage({ providers: () => Promise.resolve([]), load: () => Promise.resolve(class {}) });
  const dialog = defineChannelsSecretDialog({ providers: () => Promise.resolve([]), load: () => Promise.resolve(class {}) });
  const remove = defineChannelsRemoveDialog({ load: () => Promise.resolve(class {}) });

  it('declares the page, a modal form dialog for the secret and the commands each window may call', () => {
    expect(page.id).toBe(CHANNELS_SETTINGS_PAGE);
    expect(dialog).toMatchObject({ id: CHANNELS_SECRET_DIALOG_ID, kind: 'form', modality: 'modal', requiredChoice: false });
    expect(dialog.commands).toEqual(['channels_set_secret']);
    expect(CHANNELS_SECRET_DIALOG_COMMANDS).toEqual(['channels_set_secret']);
    // The settings window never sets a secret; the dialog window does nothing else.
    expect(CHANNELS_SETTINGS_COMMANDS).not.toContain('channels_set_secret');
    for (const c of [...CHANNELS_SETTINGS_COMMANDS, ...CHANNELS_SECRET_DIALOG_COMMANDS]) expect(CHANNELS_COMMANDS[c]).toBeDefined();
    expect(CHANNELS_COMMANDS.channels_set_secret.windows).toEqual(['dialog']);
    // The remove confirmation is a modal decision, one window per channel, and calls nothing.
    expect(remove).toMatchObject({ id: CHANNELS_REMOVE_DIALOG_ID, kind: 'decision', modality: 'modal', instanced: true });
    expect(remove.commands).toEqual([]);
  });

  it('passes the shell’s contribution checks', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions(channelsContributions(page, [dialog, remove]))] });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('gives the secret dialog the backend and no store', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideChannelsBackend({ backend: () => new MemoryChannelsBackend() })] });
    expect(TestBed.inject(ChannelsBackend)).toBeInstanceOf(MemoryChannelsBackend);
    expect(() => TestBed.inject(ChannelsStore)).toThrow();
  });
});

describe('the home-network switch', () => {
  it('is read from the engine, off when an older engine sends none, and must be a boolean', () => {
    const { allowLan: _off, ...old } = WEBHOOK;
    expect(parseChannel(old)).toEqual({ ...WEBHOOK, allowLan: false });
    expect(parseChannel({ ...WEBHOOK, allowLan: true })).toMatchObject({ allowLan: true });
    expect(parseChannel({ ...WEBHOOK, allowLan: 'yes' })).toBeNull();
  });

  it('goes through the draft both ways', () => {
    expect(EMPTY_DRAFT.allowLan).toBe(false);
    const on = { ...WEBHOOK, allowLan: true };
    expect(draftFromChannel(on).allowLan).toBe(true);
    expect(draftToChannel(draftFromChannel(on), []).allowLan).toBe(true);
  });

  it('is what the in-memory engine checks a private address against', () => {
    const lan = (allowLan: boolean, url: string): ChannelConfig => ({ ...WEBHOOK, allowLan, transport: { type: 'webhook', url, auth: false } });
    for (const url of ['http://192.168.1.20/x', 'http://nas.local/x', 'https://10.0.0.5/x']) {
      expect(settingsProblem({ channels: [lan(false, url)] })).toMatch(/home network/);
      expect(settingsProblem({ channels: [lan(true, url)] })).toBeNull();
    }
    expect(settingsProblem({ channels: [lan(true, 'http://hooks.example.com/x')] })).toMatch(/https/);
    expect(settingsProblem({ channels: [lan(false, 'http://localhost:8080/x')] })).toBeNull();
  });
});

describe('declared events', () => {
  const E = [
    { name: 'a', group: 'One', description: 'A.' },
    { name: 'b', group: 'Two', description: 'B.' },
    { name: 'c', group: 'One', description: 'C.' },
    { name: 'a', group: 'Two', description: 'repeat' },
  ];

  it('are grouped in order of first appearance, a repeated name once', () => {
    expect(groupEvents(E).map((g) => [g.group, g.events.map((e) => e.name)])).toEqual([['One', ['a', 'c']], ['Two', ['b']]]);
    expect(groupEvents([])).toEqual([]);
  });

  it('leave the names not declared, and toggling adds last or removes', () => {
    expect(unknownEvents(['a', 'x', 'b', 'y'], E)).toEqual(['x', 'y']);
    expect(toggleEvent(['a', 'x'], 'b', true)).toEqual(['a', 'x', 'b']);
    expect(toggleEvent(['a', 'x', 'b'], 'x', false)).toEqual(['a', 'b']);
    expect(toggleEvent(['a'], 'a', true)).toEqual(['a']);
  });

  it('default to none, and the page definition provides them next to the app’s providers', async () => {
    expect(TestBed.inject(CHANNEL_EVENTS, null)).toBeNull();
    const providers = async () => [];
    const load = async () => class {};
    const plain = defineChannelsSettingsPage({ providers, load });
    expect(plain.providers).toBe(providers);
    const declared = defineChannelsSettingsPage({ providers, load, events: E });
    expect(await declared.providers!()).toEqual([{ provide: CHANNEL_EVENTS, useValue: E }]);
  });
});
