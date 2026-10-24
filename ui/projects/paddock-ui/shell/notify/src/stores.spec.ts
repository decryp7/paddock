import { TestBed } from '@angular/core/testing';
import { MemoryNotificationCentre, NotificationsBackend, ServiceHealthBackend, type Unlisten } from './backends';
import { NotificationsStore, localCause } from './notifications.store';
import { ServiceHealthStore } from './service-health.store';
import { cause, notifyBed, settle } from './spec-helpers';
import type { Notice, NoticesEvent, NoticesSnapshot, ServiceHealthSnapshot } from './wire.types';

class SlowHealth extends ServiceHealthBackend {
  resolveSnapshot: (s: ServiceHealthSnapshot) => void = () => undefined;
  rejectSnapshot: (e: unknown) => void = () => undefined;
  handler: ((s: ServiceHealthSnapshot) => void) | null = null;
  unlistened = 0;
  listenFails = false;
  snapshot(): Promise<ServiceHealthSnapshot> {
    return new Promise((res, rej) => {
      this.resolveSnapshot = res;
      this.rejectSnapshot = rej;
    });
  }
  repair(): Promise<ServiceHealthSnapshot> {
    return Promise.reject({ code: 'refused', message: 'The store is being repaired already.' });
  }
  listen(handler: (s: ServiceHealthSnapshot) => void): Promise<Unlisten> {
    if (this.listenFails) return Promise.reject(new Error('no events'));
    this.handler = handler;
    return Promise.resolve(() => void this.unlistened++);
  }
}

const down = (revision: number, state: 'down' | 'ok' = 'down'): ServiceHealthSnapshot => ({
  revision,
  services: [{ service: 'store', label: 'the log store', state, cause: state === 'down' ? cause('It stopped.') : null, repair: state === 'down' ? 'restart' : null }],
});

describe('ServiceHealthStore', () => {
  it('claims nothing healthy until the first snapshot', async () => {
    const slow = new SlowHealth();
    notifyBed({ health: slow });
    const store = TestBed.inject(ServiceHealthStore);
    await settle();
    expect(store.isReady()).toBe(false);
    expect(store.isServing('store')).toBe(false);
    expect(store.isServing('anything')).toBe(false);
    expect(store.health('store')).toBeNull();
    slow.resolveSnapshot({ revision: 0, services: [] });
    await settle();
    expect(store.isReady()).toBe(true);
    expect(store.isServing('anything')).toBe(true);
  });

  it('an event before the answer counts as the first snapshot; the newest revision wins', async () => {
    const slow = new SlowHealth();
    notifyBed({ health: slow });
    const store = TestBed.inject(ServiceHealthStore);
    await settle();
    slow.handler?.(down(3));
    expect(store.isReady()).toBe(true);
    expect(store.problems().map((p) => p.service)).toEqual(['store']);
    expect(store.isServing('store')).toBe(false);
    slow.resolveSnapshot(down(2, 'ok'));
    await settle();
    expect(store.problems()).toHaveLength(1);
    slow.handler?.(down(4, 'ok'));
    expect(store.problems()).toHaveLength(0);
    expect(store.isServing('store')).toBe(true);
  });

  it('a failed first read is an error state, not "healthy"; a refused repair is kept per service', async () => {
    const slow = new SlowHealth();
    notifyBed({ health: slow });
    const store = TestBed.inject(ServiceHealthStore);
    await settle();
    slow.rejectSnapshot(new Error('the shell is starting'));
    await settle();
    expect(store.requestError()).toBe('the shell is starting');
    expect(store.isReady()).toBe(false);
    await store.repair('store');
    expect(store.repairErrors()).toEqual({ store: 'The store is being repaired already.' });
    store.showDetails();
    expect(store.detailsRequested()).toBe(1);
  });

  it('repairs through the backend and stops listening when destroyed', async () => {
    const { health } = notifyBed();
    const store = TestBed.inject(ServiceHealthStore);
    await settle();
    health.fail('store', cause('It stopped.'));
    expect(store.problems()).toHaveLength(1);
    await store.repair('store');
    expect(store.health('store')?.state).toBe('repairing');
    health.settle('store');
    expect(store.problems()).toHaveLength(0);
    TestBed.resetTestingModule();
    health.fail('store', cause('Again.'));
  });

  it('a listen that fails is logged; a listen answered after destroy is undone', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const slow = new SlowHealth();
    slow.listenFails = true;
    notifyBed({ health: slow });
    TestBed.inject(ServiceHealthStore);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    TestBed.resetTestingModule();
    const late = new SlowHealth();
    notifyBed({ health: late });
    TestBed.inject(ServiceHealthStore);
    TestBed.resetTestingModule();
    await settle();
    expect(late.unlistened).toBe(1);
    warn.mockRestore();
  });
});

class FailingCentre extends NotificationsBackend {
  fail = true;
  list(): Promise<NoticesSnapshot> {
    return this.fail ? Promise.reject(new Error('the centre is gone')) : Promise.resolve({ revision: 0, notices: [], unread: 0 });
  }
  markRead(): Promise<boolean> {
    return Promise.reject('no');
  }
  markAllRead(): Promise<number> {
    return Promise.reject(new Error('no'));
  }
  dismiss(): Promise<boolean> {
    return Promise.reject(new Error('no'));
  }
  postNotice(): Promise<Notice> {
    return Promise.reject(new Error('the centre is gone'));
  }
  listen(_h: (e: NoticesEvent) => void): Promise<Unlisten> {
    return Promise.reject(new Error('no events'));
  }
  onOpen(): Promise<Unlisten> {
    return Promise.resolve(() => undefined);
  }
}

describe('NotificationsStore', () => {
  it('previews what the engine posts, once per cause, and drops a toast whose message went', async () => {
    const { centre } = notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    expect(store.isLoaded()).toBe(true);
    centre.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'Export failed' });
    centre.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'Export failed' });
    centre.post({ cause: 'job:2', source: 'job', severity: 'info', title: 'quiet', quiet: true });
    expect(store.toasts().map((t) => [t.cause, t.count])).toEqual([['job:1', 2]]);
    expect(store.unread()).toBe(2);
    expect(store.groups().map((g) => [g.source, g.notices.length])).toEqual([['job', 2]]);
    centre.resolve('job:1');
    expect(store.toasts()).toEqual([]);
  });

  it('a read posted message is not toasted', async () => {
    notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    const n = { id: 9, source: 's', severity: 'info', title: 't', body: '', code: null, at: 1, firstAt: 1, count: 1, read: true, primary: null, secondary: null, tertiary: null, link: { window: null, jobId: null, path: null }, cause: 'c', scope: 'app' } as const;
    store.apply({ posted: n, revision: 1, notices: [n], unread: 0 });
    expect(store.toasts()).toEqual([]);
  });

  it('posts the UI’s own notices to the engine, which keys them and tells every window', async () => {
    const { centre } = notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    await store.post({ level: 'error', message: 'Could not save', kind: 'sd.save' });
    await store.post({ level: 'error', message: 'Could not save', kind: 'sd.save' });
    await store.post({ level: 'info', message: 'Saved' });
    expect(store.toasts().map((t) => [t.cause, t.count, t.id > 0])).toEqual([
      ['ui:sd.save:Could not save', 2, true],
      ['ui:info:Saved', 1, true],
    ]);
    expect((await centre.list()).notices.map((n) => n.source).sort()).toEqual(['app', 'sd.save']);
    expect(store.local()).toEqual([]);
  });

  it('keeps a notice itself when the engine cannot take it', async () => {
    const failing = new FailingCentre();
    failing.fail = false;
    notifyBed({ centre: failing });
    const store = TestBed.inject(NotificationsStore);
    await settle();
    await store.post({ level: 'warning', message: 'Offline' });
    expect(store.local().map((n) => [n.cause, n.id < 0])).toEqual([['ui:warning:Offline', true]]);
    await expect(new MemoryNotificationCentre().postNotice({ severity: 'info', title: '  ' })).rejects.toThrow('title');
  });

  it('merges the notices it keeps by kind and text, with a count, and lists them', async () => {
    const { clock } = notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    const first = store.postLocal({ level: 'error', message: 'Could not save', kind: 'sd.save' });
    clock.time += 5;
    store.postLocal({ level: 'error', message: 'Could not save', kind: 'sd.save' });
    clock.time += 5;
    store.postLocal({ level: 'info', message: 'Saved' });
    expect(first.id).toBeLessThan(0);
    expect(localCause({ level: 'info', message: 'Saved' })).toBe('ui:info:Saved');
    expect(store.toasts().map((t) => [t.cause, t.count])).toEqual([
      ['ui:sd.save:Could not save', 2],
      ['ui:info:Saved', 1],
    ]);
    expect(store.all().map((n) => n.source)).toEqual(['app', 'sd.save']);
    expect(store.unread()).toBe(2);
    await store.markRead(first.id);
    expect(store.unread()).toBe(1);
    store.setRead('unread');
    expect(store.notices().map((n) => n.title)).toEqual(['Saved']);
    store.setRead('all');
    await store.markAllRead();
    expect(store.unread()).toBe(0);
    await store.dismiss(first.id);
    expect(store.all().map((n) => n.title)).toEqual(['Saved']);
    expect(store.toasts().map((t) => t.title)).toEqual(['Saved']);
    store.dismissToast('ui:info:Saved');
    expect(store.toasts()).toEqual([]);
    await store.dismiss(12345);
  });

  it('marks, dismisses and marks all read in the engine', async () => {
    const { centre } = notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    const a = centre.post({ cause: 'a', source: 's', severity: 'info', title: 'A' });
    const b = centre.post({ cause: 'b', source: 's', severity: 'info', title: 'B' });
    await store.markRead(a.id);
    expect(store.unread()).toBe(1);
    await store.markAllRead();
    expect(store.unread()).toBe(0);
    await store.markAllRead();
    await store.dismiss(b.id);
    expect(store.all().map((n) => n.title)).toEqual(['A']);
  });

  it('an OS notification click focuses its message', async () => {
    const { centre } = notifyBed();
    const store = TestBed.inject(NotificationsStore);
    await settle();
    centre.open(5);
    expect(store.focus()).toBe(5);
    store.setFocus(null);
    expect(store.focus()).toBeNull();
  });

  it('a failing centre is an error with Retry, never a throw', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const failing = new FailingCentre();
    notifyBed({ centre: failing });
    const store = TestBed.inject(NotificationsStore);
    await settle();
    expect(store.requestError()).toBe('the centre is gone');
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    store.apply({ posted: null, revision: 1, notices: [{ id: 1, source: 's', severity: 'info', title: 't', body: '', code: null, at: 1, firstAt: 1, count: 1, read: false, primary: null, secondary: null, tertiary: null, link: { window: null, jobId: null, path: null }, cause: 'c', scope: 'app' }], unread: 1 });
    await store.markRead(1);
    expect(store.requestError()).toBe('no');
    await store.dismiss(1);
    await store.markAllRead();
    failing.fail = false;
    await store.load();
    expect(store.requestStatus()).toBe('loaded');
    warn.mockRestore();
  });

  it('a message posted while the first list is in flight stays in the centre: an older list never overwrites it', async () => {
    // The list is read before the post and answered after it (the IPC reply crosses the event).
    const centre = new MemoryNotificationCentre();
    let answer: () => void = () => undefined;
    vi.spyOn(centre, 'list').mockImplementation(() => {
      const before = MemoryNotificationCentre.prototype.list.call(centre);
      return new Promise((resolve) => (answer = () => resolve(before)));
    });
    notifyBed({ centre });
    const store = TestBed.inject(NotificationsStore);
    await settle();
    centre.post({ cause: 'settings:file', source: 'settings', severity: 'warning', title: 'Settings were reset to defaults' });
    expect(store.toasts().map((t) => t.cause)).toEqual(['settings:file']);
    answer();
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.all().map((n) => n.cause)).toEqual(['settings:file']);
    expect(store.unread()).toBe(1);
    // A Retry that answers with the current list is taken.
    centre.resolve('settings:file');
    vi.mocked(centre.list).mockRestore();
    await store.load();
    expect(store.all()).toEqual([]);
  });

  it('stops listening when destroyed, also when the listen answers late', async () => {
    const centre = new MemoryNotificationCentre();
    const off = vi.fn();
    vi.spyOn(centre, 'listen').mockResolvedValue(off);
    notifyBed({ centre });
    TestBed.inject(NotificationsStore);
    TestBed.resetTestingModule();
    await settle();
    expect(off).toHaveBeenCalled();
  });
});

describe('NOTIFY_CLOCK (default)', () => {
  it('reads the time and ticks until stopped', async () => {
    const { NOTIFY_CLOCK } = await import('./notify-config');
    vi.useFakeTimers();
    try {
      TestBed.configureTestingModule({});
      const clock = TestBed.inject(NOTIFY_CLOCK);
      expect(clock.now()).toBe(Date.now());
      const tick = vi.fn();
      const stop = clock.every(100, tick);
      vi.advanceTimersByTime(250);
      stop();
      vi.advanceTimersByTime(500);
      expect(tick).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
