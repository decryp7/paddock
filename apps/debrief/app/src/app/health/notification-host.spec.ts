import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { all, buttonByText, el, settle, text } from '../../testing/helpers';
import { MAX_VISIBLE, TOAST_MS, HostNotification, NOTIFICATION_SOURCES, NotificationSource, persists, visibleNotifications } from './notification-host.model';
import { NotificationHost } from './notification-host';

const n = (cause: string, extra: Partial<HostNotification> = {}): HostNotification => ({ cause, severity: 'info', title: cause, dismiss: () => undefined, ...extra });

describe('visibleNotifications (the one display rule)', () => {
  it('one toast per cause: the same cause three times is one, with the latest content, where it first appeared', () => {
    const out = visibleNotifications([n('a', { title: 'first' }), n('b'), n('a', { title: 'third', count: 3 })]);
    expect(out.map((x) => [x.cause, x.title])).toEqual([
      ['a', 'third'],
      ['b', 'b'],
    ]);
  });

  it('never toasts a cause the status chip states', () => {
    expect(visibleNotifications([n('health:store'), n('job:1')]).map((x) => x.cause)).toEqual(['job:1']);
  });

  it('shows at most the newest few', () => {
    const many = ['a', 'b', 'c', 'd', 'e'].map((c) => n(c));
    expect(visibleNotifications(many).map((x) => x.cause)).toEqual(['c', 'd', 'e']);
    expect(MAX_VISIBLE).toBe(3);
  });

  it('errors and critical failures persist, the rest time out', () => {
    expect((['info', 'success', 'warning', 'error', 'critical'] as const).map((severity) => persists({ severity }))).toEqual([false, false, false, true, true]);
    expect(persists({ severity: 'info', sticky: true })).toBe(true);
  });
});

describe('NotificationHost', () => {
  function setup(items: HostNotification[]) {
    const list = signal<readonly HostNotification[]>(items);
    const source: NotificationSource = { items: list };
    TestBed.configureTestingModule({ providers: [{ provide: NOTIFICATION_SOURCES, multi: true, useValue: source }] });
    const f = TestBed.createComponent(NotificationHost);
    return { f, list };
  }

  it('shows the title, the body, a count for a repeated cause, and updates in place', async () => {
    const { f, list } = setup([n('job:1', { title: 'Export failed', body: 'Disk full', count: 1 })]);
    await settle(f);
    expect(text(f)).toContain('Export failed');
    expect(text(f)).toContain('Disk full');
    expect(f.nativeElement.querySelector('[data-testid=notification-count]')).toBeNull();
    list.set([n('job:1', { title: 'Export failed', body: 'Disk full', count: 3 })]);
    await settle(f);
    expect(all(f, '[data-testid=notification]')).toHaveLength(1);
    expect(el(f, '[data-testid=notification-count]').textContent).toContain('×3');
  });

  it('collapses technical details behind Details', async () => {
    const { f } = setup([n('a', { details: 'stack: boom' })]);
    await settle(f);
    expect(f.nativeElement.querySelector('[data-testid=notification-details]')).toBeNull();
    const toggle = el<HTMLButtonElement>(f, '[data-testid=notification-details-toggle]');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    await settle(f);
    expect(el(f, '[data-testid=notification-details]').textContent).toContain('stack: boom');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
  });

  it('errors are alerts, others status; severity is on the element', async () => {
    const { f } = setup([n('e', { severity: 'error' }), n('i')]);
    await settle(f);
    const toasts = all(f, '[data-testid=notification]');
    // The newest comes first (see 'the newest toast enters at the top').
    expect(toasts.map((t) => [t.getAttribute('role'), t.getAttribute('data-severity')])).toEqual([
      ['status', 'info'],
      ['alert', 'error'],
    ]);
  });

  it('runs an action; a failing action keeps the toast and says why', async () => {
    const ok = vi.fn();
    const { f } = setup([n('a', { actions: [{ label: 'Do it', run: ok }] }), n('b', { actions: [{ label: 'Break', run: () => Promise.reject(new Error('nope')) }] })]);
    await settle(f);
    buttonByText(f, 'Do it').click();
    expect(ok).toHaveBeenCalled();
    buttonByText(f, 'Break').click();
    await settle(f);
    expect(text(f)).toContain('nope');
  });

  it('the dismiss button goes to the source', async () => {
    const dismiss = vi.fn();
    const { f } = setup([n('a', { dismiss })]);
    await settle(f);
    el<HTMLButtonElement>(f, '[data-testid=notification-dismiss]').click();
    expect(dismiss).toHaveBeenCalledTimes(1);
  });

  it('merges several sources by cause and hides health causes', async () => {
    const a: NotificationSource = { items: signal([n('x', { title: 'from a' })]) };
    const b: NotificationSource = { items: signal([n('x', { title: 'from b' }), n('health:store')]) };
    TestBed.configureTestingModule({
      providers: [
        { provide: NOTIFICATION_SOURCES, multi: true, useValue: a },
        { provide: NOTIFICATION_SOURCES, multi: true, useValue: b },
      ],
    });
    const f = TestBed.createComponent(NotificationHost);
    await settle(f);
    expect(all(f, '[data-testid=notification]')).toHaveLength(1);
    expect(text(f)).toContain('from b');
  });

  it('renders nothing without sources', async () => {
    TestBed.configureTestingModule({});
    const f = TestBed.createComponent(NotificationHost);
    await settle(f);
    expect(all(f, '[data-testid=notification]')).toHaveLength(0);
  });

  describe('the stack does not move under the pointer', () => {
    const order = (f: ReturnType<typeof setup>['f']) => all(f, '[data-testid=notification]').map((t) => t.getAttribute('data-cause'));

    it('the newest toast enters at the top: the ones already there keep their place and their elements', async () => {
      const { f, list } = setup([n('a')]);
      await settle(f);
      const before = el(f, '[data-cause=a]');
      list.set([n('a'), n('b')]);
      await settle(f);
      expect(order(f)).toEqual(['b', 'a']);
      expect(el(f, '[data-cause=a]')).toBe(before);
    });

    it('posting a cause again updates it where it is, it does not move to the top', async () => {
      const { f, list } = setup([n('a'), n('b')]);
      await settle(f);
      list.set([n('a', { count: 2 }), n('b')]);
      await settle(f);
      expect(order(f)).toEqual(['b', 'a']);
      expect(el(f, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×2');
    });

    it('while the pointer is in the stack, new toasts wait, nothing is dropped past the limit, and the order stays', async () => {
      const { f, list } = setup([n('a'), n('b'), n('c')]);
      await settle(f);
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseenter'));
      list.set([n('a'), n('b'), n('c'), n('d'), n('a', { count: 5 })]);
      await settle(f);
      expect(order(f)).toEqual(['c', 'b', 'a']);
      expect(el(f, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×5'); // updated in place
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseleave'));
      await settle(f);
      expect(order(f)).toEqual(['d', 'c', 'b']);
    });

    it('while the pointer is in the stack, a toast that goes away still goes away at once', async () => {
      const { f, list } = setup([n('a'), n('b')]);
      await settle(f);
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseenter'));
      list.set([n('a'), n('b'), n('c')]);
      await settle(f);
      list.set([n('a'), n('c')]);
      await settle(f);
      expect(order(f)).toEqual(['a']);
    });

    it('a stack that has emptied under the pointer does not hold back what arrives next', async () => {
      const { f, list } = setup([n('a')]);
      await settle(f);
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseenter'));
      list.set([]);
      await settle(f);
      list.set([n('z')]);
      await settle(f);
      expect(order(f)).toEqual(['z']);
    });
  });

  describe('time', () => {
    afterEach(() => vi.useRealTimers());

    async function timed(items: HostNotification[]) {
      vi.useFakeTimers();
      const r = setup(items);
      r.f.detectChanges();
      await vi.advanceTimersByTimeAsync(0);
      r.f.detectChanges();
      return r;
    }

    it('a toast that times out is dismissed in the source after TOAST_MS; errors, critical and sticky ones stay', async () => {
      const gone: string[] = [];
      const mk = (cause: string, extra: Partial<HostNotification> = {}) => n(cause, { dismiss: () => gone.push(cause), ...extra });
      await timed([mk('e', { severity: 'error' }), mk('s', { sticky: true }), mk('w', { severity: 'warning' })]);
      await vi.advanceTimersByTimeAsync(TOAST_MS - 1);
      expect(gone).toEqual([]);
      await vi.advanceTimersByTimeAsync(1);
      expect(gone).toEqual(['w']);
      await vi.advanceTimersByTimeAsync(TOAST_MS * 10);
      expect(gone).toEqual(['w']);
    });

    it('a toast can ask for its own time', async () => {
      const gone: string[] = [];
      await timed([n('a', { ttl: 10_000, dismiss: () => gone.push('a') })]);
      await vi.advanceTimersByTimeAsync(TOAST_MS + 1);
      expect(gone).toEqual([]);
      await vi.advanceTimersByTimeAsync(2_000);
      expect(gone).toEqual(['a']);
    });

    it('posting the cause again starts the time over', async () => {
      const gone: string[] = [];
      const dismiss = () => gone.push('a');
      const { f, list } = await timed([n('a', { stamp: 1, dismiss })]);
      await vi.advanceTimersByTimeAsync(TOAST_MS - 1000);
      list.set([n('a', { stamp: 2, count: 2, dismiss })]);
      f.detectChanges();
      await vi.advanceTimersByTimeAsync(TOAST_MS - 1000);
      expect(gone).toEqual([]);
      await vi.advanceTimersByTimeAsync(1000);
      expect(gone).toEqual(['a']);
    });

    it('holds a toast while the pointer or focus is on it and times it out once both have left', async () => {
      const gone: string[] = [];
      const { f } = await timed([n('a', { dismiss: () => gone.push('a'), actions: [{ label: 'Go', run: () => undefined }] })]);
      const stack = el(f, '[data-testid=notification-stack]');
      stack.dispatchEvent(new MouseEvent('mouseenter'));
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]);
      el<HTMLButtonElement>(f, '[data-testid=notification-action]').focus();
      stack.dispatchEvent(new MouseEvent('mouseleave'));
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]); // focus is still inside
      el<HTMLButtonElement>(f, '[data-testid=notification-action]').blur();
      stack.dispatchEvent(new FocusEvent('focusout', { relatedTarget: document.body }));
      await vi.advanceTimersByTimeAsync(TOAST_MS - 1);
      expect(gone).toEqual([]);
      await vi.advanceTimersByTimeAsync(1);
      expect(gone).toEqual(['a']);
    });

    it('the pointer on one toast holds the time of every toast in the stack', async () => {
      const gone: string[] = [];
      const { f } = await timed([n('a', { dismiss: () => gone.push('a') }), n('b', { dismiss: () => gone.push('b') })]);
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseenter'));
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]);
      el(f, '[data-testid=notification-stack]').dispatchEvent(new MouseEvent('mouseleave'));
      await vi.advanceTimersByTimeAsync(TOAST_MS);
      expect(gone.sort()).toEqual(['a', 'b']);
    });

    it('a toast that went away loses its timer', async () => {
      const gone: string[] = [];
      const { f, list } = await timed([n('a', { dismiss: () => gone.push('a') })]);
      list.set([]);
      f.detectChanges();
      await vi.advanceTimersByTimeAsync(TOAST_MS * 2);
      expect(gone).toEqual([]);
    });
  });
});
