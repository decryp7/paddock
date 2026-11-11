import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { all, buttonByText, el, settle, text } from '../../testing/helpers';
import { HOLD_RELEASE_MS, MAX_VISIBLE, TOAST_MS, HostNotification, NOTIFICATION_SOURCES, NotificationSource, persists, visibleNotifications } from './notification-host.model';
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

    it('a toast that went away loses its timer', async () => {
      const gone: string[] = [];
      const { f, list } = await timed([n('a', { dismiss: () => gone.push('a') })]);
      list.set([]);
      f.detectChanges();
      await vi.advanceTimersByTimeAsync(TOAST_MS * 2);
      expect(gone).toEqual([]);
    });
  });
  describe('the stack does not move under the pointer', () => {
    afterEach(() => vi.useRealTimers());

    const order = (f: ComponentFixture<unknown>) => all(f, '[data-testid=notification]').map((t) => t.getAttribute('data-cause'));
    /** Lets effects, the render and the release check (after the render) run on fake timers. */
    async function flush(f: ComponentFixture<unknown>) {
      for (let i = 0; i < 3; i++) {
        f.detectChanges();
        TestBed.tick();
        await vi.advanceTimersByTimeAsync(0);
      }
    }
    async function boot(items: HostNotification[]) {
      vi.useFakeTimers();
      const r = setup(items);
      await flush(r.f);
      return r;
    }
    /** The pointer on the stack's toasts. jsdom has no hover: the stack's `:hover` answers what the test says. */
    function pointer(f: ComponentFixture<unknown>) {
      const stack = el(f, '[data-testid=notification-stack]');
      let over = false;
      const real = stack.matches.bind(stack);
      vi.spyOn(stack, 'matches').mockImplementation((selector: string) => (selector === ':hover' ? over : real(selector)));
      return {
        enter: () => {
          over = true;
          el(f, '[data-testid=notification]').dispatchEvent(new Event('pointerover', { bubbles: true }));
        },
        leave: async () => {
          over = false;
          el(f, '[data-testid=notification]').dispatchEvent(new Event('pointerout', { bubbles: true }));
          await vi.advanceTimersByTimeAsync(HOLD_RELEASE_MS);
        },
      };
    }
    const focusIn = (button: HTMLElement) => {
      button.focus();
      button.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    };
    const focusOut = (button: HTMLElement) => {
      button.blur();
      button.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: null }));
    };

    it('the newest toast enters at the top: the ones already there keep their elements', async () => {
      const { f, list } = await boot([n('a')]);
      const before = el(f, '[data-cause=a]');
      list.set([n('a'), n('b')]);
      await flush(f);
      expect(order(f)).toEqual(['b', 'a']);
      expect(el(f, '[data-cause=a]')).toBe(before);
    });

    it('posting a cause again updates it where it is', async () => {
      const { f, list } = await boot([n('a'), n('b')]);
      list.set([n('a', { count: 2 }), n('b')]);
      await flush(f);
      expect(order(f)).toEqual(['b', 'a']);
      expect(el(f, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×2');
    });

    it('while the pointer is in the stack, a newcomer shows on top, and nothing is dropped or re-ordered until it leaves', async () => {
      const { f, list } = await boot([n('a'), n('b'), n('c')]);
      const mouse = pointer(f);
      mouse.enter();
      list.set([n('a'), n('b'), n('c'), n('d'), n('a', { count: 5 })]);
      await flush(f);
      expect(order(f)).toEqual(['d', 'c', 'b', 'a']); // past the limit of three, but the ones on screen stay
      expect(el(f, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×5'); // updated in place
      await mouse.leave();
      await flush(f);
      expect(order(f)).toEqual(['d', 'c', 'b']);
    });

    it('a new persistent toast shows while the stack is held', async () => {
      const { f, list } = await boot([n('a'), n('b')]);
      pointer(f).enter();
      list.set([n('a'), n('b'), n('boom', { severity: 'critical' })]);
      await flush(f);
      expect(order(f)).toEqual(['boom', 'b', 'a']);
      expect(el(f, '[data-cause=boom]').getAttribute('role')).toBe('alert');
    });

    it('while the pointer is in the stack, a toast that goes away still goes away at once', async () => {
      const { f, list } = await boot([n('a'), n('b')]);
      pointer(f).enter();
      list.set([n('a'), n('b'), n('c')]);
      await flush(f);
      list.set([n('a'), n('c')]);
      await flush(f);
      expect(order(f)).toEqual(['c', 'a']);
    });

    it('focus inside a toast that is removed releases the hold', async () => {
      const { f, list } = await boot([n('a'), n('b'), n('c')]);
      focusIn(el(f, '[data-cause=a] [data-testid=notification-dismiss]'));
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await flush(f);
      expect(order(f)).toEqual(['d', 'c', 'b', 'a']); // held: nothing dropped
      list.set([n('b'), n('c'), n('d'), n('e')]); // a goes, and with it the focus
      await flush(f);
      expect(order(f)).toEqual(['e', 'd', 'c']); // released: the limit applies again
    });

    it('focus leaving the stack without a pointer event releases the hold', async () => {
      const { f, list } = await boot([n('a'), n('b'), n('c')]);
      const button = el<HTMLButtonElement>(f, '[data-cause=b] [data-testid=notification-dismiss]');
      focusIn(button);
      focusOut(button);
      await vi.advanceTimersByTimeAsync(HOLD_RELEASE_MS);
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await flush(f);
      expect(order(f)).toEqual(['d', 'c', 'b']);
    });

    it('moving from one toast to the next across the gap does not release the hold', async () => {
      const { f, list } = await boot([n('a'), n('b'), n('c')]);
      const mouse = pointer(f);
      mouse.enter();
      el(f, '[data-testid=notification]').dispatchEvent(new Event('pointerout', { bubbles: true })); // leaves a toast into the gap
      await vi.advanceTimersByTimeAsync(HOLD_RELEASE_MS - 1);
      mouse.enter(); // and enters the next one
      await vi.advanceTimersByTimeAsync(HOLD_RELEASE_MS);
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await flush(f);
      expect(order(f)).toEqual(['d', 'c', 'b', 'a']);
    });

    it('holds a toast while the pointer or focus is in the stack and times it out once both have left', async () => {
      const gone: string[] = [];
      const { f } = await boot([n('a', { dismiss: () => gone.push('a'), actions: [{ label: 'Go', run: () => undefined }] })]);
      const mouse = pointer(f);
      mouse.enter();
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]);
      const go = el<HTMLButtonElement>(f, '[data-testid=notification-action]');
      focusIn(go);
      await mouse.leave();
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]); // focus is still inside
      focusOut(go);
      await vi.advanceTimersByTimeAsync(HOLD_RELEASE_MS);
      await vi.advanceTimersByTimeAsync(TOAST_MS - 1);
      expect(gone).toEqual([]);
      await vi.advanceTimersByTimeAsync(1);
      expect(gone).toEqual(['a']);
    });

    it('the pointer on the stack holds the time of every toast in it', async () => {
      const gone: string[] = [];
      const { f } = await boot([n('a', { dismiss: () => gone.push('a') }), n('b', { dismiss: () => gone.push('b') })]);
      const mouse = pointer(f);
      mouse.enter();
      await vi.advanceTimersByTimeAsync(TOAST_MS * 5);
      expect(gone).toEqual([]);
      await mouse.leave();
      await vi.advanceTimersByTimeAsync(TOAST_MS);
      expect(gone.sort()).toEqual(['a', 'b']);
    });

    it('the stack takes no clicks itself, only the toasts do', () => {
      const css = ((NotificationHost as unknown as { ɵcmp: { styles: string[] } }).ɵcmp.styles ?? []).join('\n').replace(/\s+/g, ' ');
      expect(css).toMatch(/pointer-events: none;/);
      expect(css).toMatch(/\.toast\[[^\]]*\] \{ pointer-events: auto;/);
      expect(css).not.toMatch(/\.stack\[[^\]]*\] \{[^}]*pointer-events: auto/);
    });
  });
});
