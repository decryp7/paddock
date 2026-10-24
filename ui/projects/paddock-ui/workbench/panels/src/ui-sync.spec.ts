import { signal, type WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { PANEL_LIMITS } from '../generated/panels';
import { InPageDesktop } from './in-page';
import { FakeSyncTimers } from './testing';
import { HELLO_INTERVAL_MS, HELLO_TRIES, SYNC_TIMERS, UiSync, type SyncedValue } from './ui-sync';

/** One window of a group: its own UiSync, its own state. */
interface Win {
  readonly sync: UiSync;
  readonly query: WritableSignal<string>;
  readonly theme: WritableSignal<string>;
  readonly cursor: WritableSignal<number | null>;
  readonly applied: string[];
}

function values(w: { query: WritableSignal<string>; theme: WritableSignal<string>; cursor: WritableSignal<number | null>; applied: string[] }): SyncedValue[] {
  return [
    { key: 'q', scope: 'full', read: () => w.query(), apply: (v) => (w.applied.push(`q:${String(v)}`), typeof v === 'string' && w.query.set(v)) },
    { key: 'theme', scope: 'display', read: () => w.theme(), apply: (v) => typeof v === 'string' && w.theme.set(v) },
    { key: 'cursor', scope: 'full', read: () => w.cursor(), apply: (v) => w.cursor.set(typeof v === 'number' ? v : null), throttleMs: 50 },
  ];
}

describe('UiSync', () => {
  let desktop: InPageDesktop;
  let timers: FakeSyncTimers;
  const wins: Win[] = [];

  function open(label: string, role: 'host' | 'popout' | 'dialog', before?: (sync: UiSync) => void): Win {
    const parts = { query: signal(''), theme: signal('light'), cursor: signal<number | null>(null), applied: [] as string[] };
    const sync = TestBed.runInInjectionContext(() => new UiSync());
    const w: Win = { sync, ...parts };
    wins.push(w);
    before?.(sync);
    const list = role === 'dialog' ? values(parts).filter((v) => v.scope === 'display') : values(parts);
    sync.start(desktop.window(label), role, label, list, `o-${label}`);
    return w;
  }

  beforeEach(() => {
    desktop = new InPageDesktop();
    timers = new FakeSyncTimers();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: SYNC_TIMERS, useValue: timers }] });
    wins.length = 0;
  });

  afterEach(() => wins.forEach((w) => w.sync.stop()));

  /** Lets effects run and the in-page "IPC" (promises) deliver. */
  const flush = async () => {
    for (let i = 0; i < 4; i++) {
      TestBed.tick();
      await Promise.resolve();
      await settle();
    }
  };

  async function group() {
    const host = open('main', 'host');
    await flush();
    desktop.window('main').emitPanels({ kind: 'changed' });
    const popped = await desktop.window('main').popOut({ panel: 'pn.code' });
    const pop = open(popped.label, 'popout');
    await flush();
    return { host, pop, popLabel: popped.label };
  }

  it('a pop-out says hello and takes the host snapshot, then both follow each other without echo', async () => {
    const host = open('main', 'host');
    host.query.set('error');
    await flush();
    const popped = await desktop.window('main').popOut({ panel: 'pn.code' });
    let synced = 0;
    const pop = open(popped.label, 'popout', (s) => s.onSynced(() => synced++));
    // The host answers the hello with its state.
    await flush();
    expect(pop.query()).toBe('error');
    expect(synced).toBe(1);
    // Changes flow both ways; applying a remote value does not bounce it back.
    pop.query.set('warn');
    await flush();
    expect(host.query()).toBe('warn');
    host.query.set('info');
    await flush();
    expect(pop.query()).toBe('info');
    expect(host.applied.filter((a) => a === 'q:info')).toEqual([]);
    expect(pop.applied).toEqual(['q:error', 'q:info']);
  });

  it('stops saying hello once the host answered, and gives up after ten tries when none does', async () => {
    const hello = vi.spyOn(desktop.window('p-main-1'), 'publish');
    const lone = open('p-main-1', 'popout');
    expect(hello).toHaveBeenCalledTimes(1);
    timers.advance(HELLO_INTERVAL_MS * (HELLO_TRIES + 3));
    // No host listens: the hellos stop at the limit.
    expect(hello.mock.calls.filter((c) => (c[0].message as { t: string }).t === 'hello').length).toBe(HELLO_TRIES);
    expect(timers.pending).toBe(0);
    lone.sync.stop();
  });

  it('a dialog gets display state only, and a snapshot of display state only', async () => {
    const host = open('main', 'host');
    host.query.set('secret query');
    host.theme.set('dark');
    await flush();
    const { pop } = await group();
    const dialog = open('d-main-9', 'dialog');
    await flush();
    expect(dialog.theme()).toBe('dark');
    expect(dialog.query()).toBe('');
    expect(dialog.applied).toEqual([]);
    host.theme.set('light');
    host.query.set('other');
    await flush();
    expect(dialog.theme()).toBe('light');
    expect(dialog.query()).toBe('');
    expect(pop.query()).toBe('other');
  });

  it('a throttled value is sent at most once per window and the latest value wins', async () => {
    const { host, pop } = await group();
    const publish = vi.spyOn(desktop.window('main'), 'publish');
    for (const n of [1, 2, 3]) {
      host.cursor.set(n);
      await flush();
    }
    const sent = () => publish.mock.calls.filter((c) => (c[0].message as { key?: string }).key === 'cursor').map((c) => (c[0].message as { value: number }).value);
    expect(sent()).toEqual([1]);
    expect(pop.cursor()).toBe(1);
    timers.advance(50);
    await flush();
    expect(sent()).toEqual([1, 3]);
    expect(pop.cursor()).toBe(3);
  });

  it('a value nobody received is sent again with the next change (a host without pop-outs)', async () => {
    const host = open('main', 'host');
    await flush();
    const publish = vi.spyOn(desktop.window('main'), 'publish');
    host.query.set('a');
    await flush();
    host.query.set('a2');
    await flush();
    expect(publish.mock.calls.map((c) => (c[0].message as { value?: unknown }).value)).toEqual(['a', 'a2']);
  });

  it('a pop-out asks its host to run a command; a host cannot', async () => {
    const { host, pop } = await group();
    const ran: string[] = [];
    host.sync.onCommand((c, a) => void ran.push(`${c}:${a}`));
    expect(pop.sync.runInHost('showView', 'pn.log')).toBe(true);
    expect(pop.sync.runInHost('go')).toBe(true);
    await flush();
    expect(ran).toEqual(['showView:pn.log', 'go:undefined']);
    expect(host.sync.runInHost('x')).toBe(false);
  });

  it('a pop-out hears the host say goodbye', async () => {
    const { host, pop } = await group();
    let gone = 0;
    pop.sync.onHostGone(() => gone++);
    pop.sync.goodbye();
    await flush();
    expect(gone).toBe(0);
    host.sync.goodbye();
    await flush();
    expect(gone).toBe(1);
  });

  it('ignores malformed and stale messages, and a value that cannot be applied', async () => {
    const { host, pop, popLabel } = await group();
    const win = desktop.window(popLabel);
    win.emitSync(null);
    win.emitSync({ t: 'state' });
    win.emitSync({ t: 'state', origin: 'o-main', seq: 1, host: true, key: 'q', value: 'stale' });
    expect(pop.query()).not.toBe('stale');
    // A key the window does not know is ignored; an apply that throws is contained.
    win.emitSync({ t: 'state', origin: 'o-main', seq: 999, host: true, key: 'nope', value: 1 });
    const boom = open('p-main-7', 'popout');
    boom.sync.stop();
    const thrower = TestBed.runInInjectionContext(() => new UiSync());
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    thrower.start(desktop.window('p-main-5'), 'popout', 'p-main-5', [{ key: 'q', scope: 'full', read: () => 1, apply: () => { throw new Error('bad'); } }], 'o-x');
    desktop.window('p-main-5').emitSync({ t: 'state', origin: 'o-main', seq: 1000, host: true, key: 'q', value: 1 });
    expect(warn).toHaveBeenCalled();
    thrower.stop();
    warn.mockRestore();
    expect(host.query()).toBe('');
  });

  it('does not send a message over the size limit', async () => {
    const host = open('main', 'host');
    await flush();
    const publish = vi.spyOn(desktop.window('main'), 'publish');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    host.query.set('x'.repeat(PANEL_LIMITS.maxMessageBytes + 1));
    await flush();
    expect(publish).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it('forgets a failed send so the next change goes out', async () => {
    const host = open('main', 'host');
    await flush();
    const io = desktop.window('main');
    const publish = vi.spyOn(io, 'publish').mockRejectedValueOnce(new Error('offline'));
    host.query.set('a');
    await flush();
    host.query.set('a');
    await flush();
    expect(publish).toHaveBeenCalledTimes(1);
    host.query.set('b');
    await flush();
    expect(publish).toHaveBeenCalledTimes(2);
  });

  it('stopping ends the effects, timers and listening; a second start replaces the first', async () => {
    const host = open('main', 'host');
    expect(host.sync.started).toBe(true);
    host.sync.stop();
    expect(host.sync.started).toBe(false);
    const publish = vi.spyOn(desktop.window('main'), 'publish');
    host.query.set('after stop');
    await flush();
    expect(publish).not.toHaveBeenCalled();
    host.sync.stop();
    expect(host.sync.runInHost('x')).toBe(false);
    host.sync.goodbye();
  });
});
