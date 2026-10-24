import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MAX_VISIBLE, NOTIFY_CLOCK, TOAST_MS } from './notify-config';
import { NOTIFICATION_SOURCES, persists, visibleNotifications, type HostNotification, type NotificationSource } from './notification-host.model';
import { PdNotificationHost } from './notification-host';
import { all, FakeClock, one } from './spec-helpers';

const n = (cause: string, extra: Partial<HostNotification> = {}): HostNotification => ({ cause, severity: 'info', title: cause, dismiss: () => undefined, ...extra });

describe('visibleNotifications (the one display rule)', () => {
  it('one toast per cause: the same cause three times is one, in the newest position', () => {
    const out = visibleNotifications([n('a', { title: 'first' }), n('b'), n('a', { title: 'third', count: 3 })]);
    expect(out.map((x) => [x.cause, x.title])).toEqual([
      ['b', 'b'],
      ['a', 'third'],
    ]);
  });

  it('never toasts a cause the status chip states', () => {
    expect(visibleNotifications([n('health:store'), n('job:1')]).map((x) => x.cause)).toEqual(['job:1']);
  });

  it('shows at most the newest few', () => {
    expect(visibleNotifications(['a', 'b', 'c', 'd', 'e'].map((c) => n(c))).map((x) => x.cause)).toEqual(['c', 'd', 'e']);
    expect(MAX_VISIBLE).toBe(3);
  });

  it('errors and critical failures persist, the rest time out', () => {
    expect((['info', 'success', 'warning', 'error', 'critical'] as const).map((severity) => persists({ severity }))).toEqual([false, false, false, true, true]);
    expect(persists({ severity: 'info', sticky: true })).toBe(true);
  });
});

describe('pd-notification-host', () => {
  function setup(items: HostNotification[], withSource = true) {
    const list = signal<readonly HostNotification[]>(items);
    const source: NotificationSource = { items: list };
    const clock = new FakeClock();
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: NOTIFY_CLOCK, useValue: clock }, ...(withSource ? [{ provide: NOTIFICATION_SOURCES, multi: true, useValue: source }] : [])],
    });
    const f = TestBed.createComponent(PdNotificationHost);
    return { f, list, clock, el: f.nativeElement as HTMLElement };
  }

  it('shows nothing without sources', async () => {
    const { f, el } = setup([], false);
    await f.whenStable();
    expect(all(el, '[data-testid=notification]')).toHaveLength(0);
  });

  it('shows the title, the body, a count for a repeated cause, and updates in place', async () => {
    const { f, list, el } = setup([n('job:1', { title: 'Export failed', body: 'Disk full', count: 1 })]);
    await f.whenStable();
    expect(el.textContent).toContain('Export failed');
    expect(el.textContent).toContain('Disk full');
    expect(el.querySelector('[data-testid=notification-count]')).toBeNull();
    list.set([n('job:1', { title: 'Export failed', body: 'Disk full', count: 3 })]);
    await f.whenStable();
    expect(all(el, '[data-testid=notification]')).toHaveLength(1);
    expect(one(el, '[data-testid=notification-count]').textContent).toContain('×3');
  });

  it('never shows a health cause', async () => {
    const { f, el } = setup([n('health:store', { severity: 'error' })]);
    await f.whenStable();
    expect(all(el, '[data-testid=notification]')).toHaveLength(0);
  });

  it('folds technical details behind Details, per occurrence', async () => {
    const { f, list, el } = setup([n('a', { details: 'stack: boom', instance: 1 })]);
    await f.whenStable();
    expect(el.querySelector('[data-testid=notification-details]')).toBeNull();
    const toggle = one(el, '[data-testid=notification-details-toggle]');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    await f.whenStable();
    expect(one(el, '[data-testid=notification-details]').textContent).toContain('stack: boom');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    toggle.click();
    await f.whenStable();
    expect(el.querySelector('[data-testid=notification-details]')).toBeNull();
    toggle.click();
    list.set([n('a', { details: 'stack: boom', instance: 2 })]);
    await f.whenStable();
    expect(el.querySelector('[data-testid=notification-details]')).toBeNull();
  });

  it('errors are alerts, others status; severity is on the element', async () => {
    const { f, el } = setup([n('e', { severity: 'error' }), n('i')]);
    await f.whenStable();
    expect(all(el, '[data-testid=notification]').map((t) => [t.getAttribute('role'), t.getAttribute('data-severity')])).toEqual([
      ['alert', 'error'],
      ['status', 'info'],
    ]);
  });

  it('runs an action; a failing action keeps the toast and says why', async () => {
    const ok = vi.fn();
    const { f, el } = setup([
      n('a', { severity: 'error', actions: [{ label: 'Do it', run: ok, kind: 'primary', id: 'do' }] }),
      n('b', { severity: 'error', actions: [{ label: 'Break', run: () => Promise.reject(new Error('nope')) }] }),
    ]);
    await f.whenStable();
    one(el, '[data-testid=notification-action-do]').click();
    one(el, '[data-testid=notification-action-0]').click();
    await f.whenStable();
    await Promise.resolve();
    await f.whenStable();
    expect(ok).toHaveBeenCalled();
    expect(one(el, '[data-cause=b] .err').textContent).toContain('nope');
  });

  it('dismisses on the close button', async () => {
    const dismiss = vi.fn();
    const { f, el } = setup([n('a', { dismiss })]);
    await f.whenStable();
    one(el, '[data-testid=notification-dismiss]').click();
    expect(dismiss).toHaveBeenCalled();
  });

  it('owns the time: a toast that times out goes after its ttl; errors stay', async () => {
    const info = vi.fn();
    const err = vi.fn();
    const { f, clock } = setup([n('i', { dismiss: info }), n('e', { severity: 'error', dismiss: err })]);
    await f.whenStable();
    clock.advance(TOAST_MS - 1);
    expect(info).not.toHaveBeenCalled();
    clock.advance(1);
    expect(info).toHaveBeenCalledTimes(1);
    clock.advance(TOAST_MS * 10);
    expect(err).not.toHaveBeenCalled();
  });

  it('a repeat (new stamp) starts the time over; a ttl of its own is kept', async () => {
    const dismiss = vi.fn();
    const { f, list, clock } = setup([n('i', { dismiss, stamp: 1, ttl: 1000 })]);
    await f.whenStable();
    clock.advance(900);
    list.set([n('i', { dismiss, stamp: 2, ttl: 1000 })]);
    await f.whenStable();
    clock.advance(900);
    expect(dismiss).not.toHaveBeenCalled();
    clock.advance(100);
    expect(dismiss).toHaveBeenCalled();
  });

  it('holds the time while the pointer or focus is on a toast', async () => {
    const dismiss = vi.fn();
    const { f, list, clock, el } = setup([n('i', { dismiss, ttl: 1000 })]);
    await f.whenStable();
    const toast = one(el, '[data-testid=notification]');
    toast.dispatchEvent(new MouseEvent('mouseenter'));
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    // A repeat while held stays held.
    list.set([n('i', { dismiss, ttl: 1000, stamp: 'again' })]);
    await f.whenStable();
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    // Focus moving inside the toast keeps it held.
    const button = one(el, '[data-testid=notification-dismiss]');
    toast.dispatchEvent(new FocusEvent('focusout', { relatedTarget: button }));
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    toast.dispatchEvent(new MouseEvent('mouseleave'));
    clock.advance(999);
    expect(dismiss).not.toHaveBeenCalled();
    clock.advance(1);
    expect(dismiss).toHaveBeenCalledTimes(1);
  });

  it('forgets the timer of a toast that went', async () => {
    const dismiss = vi.fn();
    const { f, list, clock } = setup([n('i', { dismiss }), n('j', { severity: 'error' })]);
    await f.whenStable();
    list.set([n('j', { severity: 'error' })]);
    await f.whenStable();
    clock.advance(TOAST_MS * 2);
    expect(dismiss).not.toHaveBeenCalled();
    expect(clock.timers.size).toBe(0);
    // A toast that turns into an error stops timing out.
    list.set([n('k', { dismiss })]);
    await f.whenStable();
    list.set([n('k', { dismiss, severity: 'error' })]);
    await f.whenStable();
    clock.advance(TOAST_MS * 2);
    expect(dismiss).not.toHaveBeenCalled();
    f.destroy();
  });
});
