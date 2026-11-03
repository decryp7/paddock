import { MemoryNotificationCentre, MemoryServiceHealth, TauriNotificationsBackend, TauriServiceHealthBackend, type NotifyIpc } from './backends';
import { cause } from './spec-helpers';
import type { NoticesEvent, ServiceHealthSnapshot } from './wire.types';

function fakeIpc(answers: Record<string, unknown>) {
  const calls: [string, unknown][] = [];
  const handlers = new Map<string, (p: unknown) => void>();
  const ipc: NotifyIpc = {
    invoke: <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      return Promise.resolve(answers[command] as T);
    },
    listen: (event, handler) => {
      handlers.set(event, handler);
      return Promise.resolve(() => void handlers.delete(event));
    },
  };
  return { ipc, calls, emit: (event: string, p: unknown) => handlers.get(event)?.(p), handlers };
}

const snapshot: ServiceHealthSnapshot = { revision: 1, services: [{ service: 'store', label: 'the log store', state: 'ok', cause: null, repair: null }] };

describe('TauriServiceHealthBackend', () => {
  it('calls the commands and parses at the edge', async () => {
    const { ipc, calls, emit } = fakeIpc({ health_snapshot: snapshot, health_repair: snapshot });
    const b = new TauriServiceHealthBackend(ipc);
    await expect(b.snapshot()).resolves.toEqual(snapshot);
    await expect(b.repair('store')).resolves.toEqual(snapshot);
    expect(calls).toEqual([['health_snapshot', undefined], ['health_repair', { service: 'store' }]]);
    const seen: ServiceHealthSnapshot[] = [];
    const off = await b.listen((s) => seen.push(s));
    emit('paddock://service-health', snapshot);
    emit('paddock://service-health', { nonsense: true });
    expect(seen).toEqual([snapshot]);
    off();
  });

  it('refuses an answer that is not the contract', async () => {
    const b = new TauriServiceHealthBackend(fakeIpc({ health_snapshot: { revision: 'x' } }).ipc);
    await expect(b.snapshot()).rejects.toThrow('cannot read');
  });
});

describe('TauriNotificationsBackend', () => {
  it('calls the commands, parses events and opens', async () => {
    const { ipc, calls, emit } = fakeIpc({ notifications_list: { revision: 0, notices: [], unread: 0 }, notifications_mark_read: true, notifications_mark_all_read: 2, notifications_dismiss: false });
    const b = new TauriNotificationsBackend(ipc);
    await expect(b.list()).resolves.toEqual({ revision: 0, notices: [], unread: 0 });
    await expect(b.markRead(4)).resolves.toBe(true);
    await expect(b.markAllRead()).resolves.toBe(2);
    await expect(b.dismiss(4)).resolves.toBe(false);
    expect(calls.map((c) => c[0])).toEqual(['notifications_list', 'notifications_mark_read', 'notifications_mark_all_read', 'notifications_dismiss']);
    expect(calls[1]?.[1]).toEqual({ id: 4 });
    await expect(b.dismissMany([1, 2])).resolves.toBe(0);
    await expect(b.dismissAll()).resolves.toBe(0);
    expect(calls.slice(4).map((c) => c[0])).toEqual(['notifications_dismiss_many', 'notifications_dismiss_all']);
    expect(calls[4]?.[1]).toEqual({ ids: [1, 2] });
    const events: NoticesEvent[] = [];
    const opened: number[] = [];
    await b.listen((e) => events.push(e));
    await b.onOpen((id) => opened.push(id));
    emit('paddock://notifications', { revision: 1, notices: [], unread: 0, posted: null });
    emit('paddock://notifications', 'junk');
    emit('paddock://notifications-open', 7);
    emit('paddock://notifications-open', 'seven');
    emit('paddock://notifications-open', 1.5);
    expect(events).toHaveLength(1);
    expect(opened).toEqual([7]);
  });

  it('hears the buttons of an OS notification that this window runs, and drops what is not one', async () => {
    const { ipc, emit } = fakeIpc({});
    const b = new TauriNotificationsBackend(ipc);
    const heard: unknown[] = [];
    await b.onAction((e) => heard.push(e));
    emit('paddock://notifications-action', { id: 4, action: { type: 'openSettings', section: 'general' } });
    emit('paddock://notifications-action', { id: 'four', action: { type: 'openSettings' } });
    emit('paddock://notifications-action', { id: 4, action: null });
    emit('paddock://notifications-action', 'junk');
    expect(heard).toEqual([{ id: 4, action: { type: 'openSettings', section: 'general' } }]);
  });

  it('reads a non-number count as none and refuses a malformed list', async () => {
    const b = new TauriNotificationsBackend(fakeIpc({ notifications_mark_all_read: null, notifications_list: null }).ipc);
    await expect(b.markAllRead()).resolves.toBe(0);
    await expect(b.list()).rejects.toThrow('notification list');
  });
});

describe('MemoryNotificationCentre', () => {
  it('merges a repeated cause (count, latest text, unread again) and lists newest first', async () => {
    let now = 10;
    const c = new MemoryNotificationCentre(() => now);
    const events: NoticesEvent[] = [];
    await c.listen((e) => events.push(e));
    const a = c.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'first' });
    now = 20;
    c.post({ cause: 'job:2', source: 'job', severity: 'info', title: 'other', quiet: true });
    await c.markRead(a.id);
    now = 30;
    const again = c.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'second', body: 'b' });
    expect(again).toMatchObject({ id: a.id, count: 2, title: 'second', body: 'b', read: false, at: 30, firstAt: 10 });
    const list = await c.list();
    expect(list.notices.map((n) => n.cause)).toEqual(['job:1', 'job:2']);
    expect(list.unread).toBe(2);
    expect(events.map((e) => e.posted?.title ?? null)).toEqual(['first', null, null, 'second']);
  });

  it('dismisses a selection and everything, counting what went', async () => {
    const c = new MemoryNotificationCentre();
    const a = c.post({ cause: 'a', source: 's', severity: 'info', title: 'A' });
    const b = c.post({ cause: 'b', source: 's', severity: 'info', title: 'B' });
    c.post({ cause: 'c', source: 's', severity: 'info', title: 'C' });
    await expect(c.dismissMany([])).resolves.toBe(0);
    await expect(c.dismissMany([999])).resolves.toBe(0);
    await expect(c.dismissMany([a.id, a.id, b.id, 999])).resolves.toBe(2);
    expect((await c.list()).notices.map((n) => n.title)).toEqual(['C']);
    await expect(c.dismissAll()).resolves.toBe(1);
    await expect(c.dismissAll()).resolves.toBe(0);
  });

  it('resolves, dismisses, marks all read and opens', async () => {
    const c = new MemoryNotificationCentre();
    const opened: number[] = [];
    const off = await c.onOpen((id) => opened.push(id));
    const n = c.post({ cause: 'a', source: 's', severity: 'info', title: 't' });
    c.post({ cause: 'b', source: 's', severity: 'info', title: 't' });
    expect(c.resolve('a')).toBe(true);
    expect(c.resolve('a')).toBe(false);
    await expect(c.markRead(n.id)).resolves.toBe(false);
    await expect(c.markAllRead()).resolves.toBe(1);
    await expect(c.markAllRead()).resolves.toBe(0);
    const b = (await c.list()).notices[0];
    await expect(c.dismiss(b?.id ?? 0)).resolves.toBe(true);
    expect((await c.list()).notices).toEqual([]);
    c.open(3);
    off();
    c.open(4);
    expect(opened).toEqual([3]);
    const ran: number[] = [];
    const offAction = await c.onAction((e) => ran.push(e.id));
    c.runAction({ id: 1, action: { type: 'x' } });
    offAction();
    c.runAction({ id: 2, action: { type: 'x' } });
    expect(ran).toEqual([1]);
  });
});

describe('MemoryServiceHealth', () => {
  it('dedupes identical failures but reports each to the centre: one change, one message with count 3', async () => {
    const centre = new MemoryNotificationCentre();
    const h = new MemoryServiceHealth([{ id: 'store', label: 'the log store' }], { centre });
    const seen: number[] = [];
    await h.listen((s) => seen.push(s.revision));
    for (let i = 0; i < 3; i++) h.fail('store', cause('It stopped.', { hint: 'Choose Restart.' }));
    expect(seen).toEqual([1]);
    const notices = (await centre.list()).notices;
    expect(notices).toHaveLength(1);
    expect(notices[0]).toMatchObject({ cause: 'health:store', count: 3, source: 'health', severity: 'error', title: 'It stopped.', body: 'Choose Restart.', primary: { type: 'repairService', service: 'store' } });
    h.ok('store');
    expect((await centre.list()).notices).toEqual([]);
  });

  it('degraded is a warning; openSettings and no repair have no button; unknown services are ignored', async () => {
    const centre = new MemoryNotificationCentre();
    const h = new MemoryServiceHealth([{ id: 'a', label: 'A' }, { id: 'b', label: 'B' }], { centre });
    h.fail('a', cause('slow'), 'retry', 'degraded');
    h.fail('b', cause('set it up'), 'openSettings');
    h.fail('nope', cause('x'));
    h.fail('a', cause('slow'), null, 'repairing');
    const byCause = Object.fromEntries((await centre.list()).notices.map((n) => [n.cause, n]));
    expect(byCause['health:a']).toMatchObject({ severity: 'warning', primary: { type: 'repairService' } });
    expect(byCause['health:b']?.primary).toBeNull();
    expect((await h.snapshot()).services.map((s) => s.state)).toEqual(['repairing', 'down']);
  });

  it('repairs: repairing at once, then the outcome on settle', async () => {
    const outcomes = ['ok', { cause: cause('Still broken.') }, { cause: cause('Gone.'), repair: null, state: 'quarantined' as const }] as const;
    let i = 0;
    const h = new MemoryServiceHealth([{ id: 's', label: 'S' }], { repairOutcome: () => outcomes[i++] ?? 'ok' });
    h.fail('s', cause('x'));
    expect((await h.repair('s')).services[0]?.state).toBe('repairing');
    await expect(h.repair('s')).rejects.toThrow('nothing to repair');
    h.settle('s');
    h.settle('s');
    expect((await h.snapshot()).services[0]?.state).toBe('ok');
    await expect(h.repair('s')).rejects.toThrow('nothing to repair for S');
    h.fail('s', cause('x'));
    await h.repair('s');
    h.settle('s');
    expect((await h.snapshot()).services[0]).toMatchObject({ state: 'down', repair: 'restart', cause: { message: 'Still broken.' } });
    await h.repair('s');
    h.settle('s');
    expect((await h.snapshot()).services[0]).toMatchObject({ state: 'quarantined', repair: null });
    await expect(h.repair('s')).rejects.toThrow('nothing to repair');
    await expect(h.repair('who')).rejects.toThrow('No service is called who.');
    expect(h.repairs).toEqual(['s', 's', 's']);
  });

  it('settles by itself with autoSettle', async () => {
    const h = new MemoryServiceHealth([{ id: 's', label: 'S' }], { autoSettle: true });
    h.fail('s', cause('x'));
    await h.repair('s');
    await new Promise((r) => setTimeout(r, 0));
    expect((await h.snapshot()).services[0]?.state).toBe('ok');
  });
});
