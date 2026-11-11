import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ToastArea } from '@decryp7/paddock-ui/shell';
import { HOLD_RELEASE_MS, MAX_VISIBLE, NOTIFY_CLOCK, TOAST_MS } from './notify-config';
import { NOTIFICATION_SOURCES, persists, visibleNotifications, type HostNotification, type NotificationSource } from './notification-host.model';
import { PdNotificationHost } from './notification-host';
import { all, FakeClock, one } from './spec-helpers';

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

  /** The pointer on the stack's toasts. jsdom has no hover: the stack's `:hover` answers what the test says. */
  function pointer(el: HTMLElement, clock: FakeClock) {
    const stack = one(el, '[data-testid=notification-stack]');
    let over = false;
    const real = stack.matches.bind(stack);
    vi.spyOn(stack, 'matches').mockImplementation((selector: string) => (selector === ':hover' ? over : real(selector)));
    const toast = () => one(el, '[data-testid=notification]');
    return {
      enter: () => {
        over = true;
        toast().dispatchEvent(new Event('pointerover', { bubbles: true }));
      },
      leave: () => {
        over = false;
        toast().dispatchEvent(new Event('pointerout', { bubbles: true }));
        clock.advance(HOLD_RELEASE_MS);
      },
    };
  }
  /** Focus moves into the toast's button (the event a browser sends) or out of it. */
  const focusIn = (button: HTMLElement) => {
    button.focus();
    button.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
  };
  const focusOut = (button: HTMLElement) => {
    button.blur();
    button.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: null }));
  };

  it('shows nothing without sources', async () => {
    const { f, el } = setup([], false);
    await f.whenStable();
    expect(all(el, '[data-testid=notification]')).toHaveLength(0);
  });

  it('lifts itself above the undo snack bar while one is open (the one toast area)', async () => {
    const { f, el } = setup([n('job:1')]);
    await f.whenStable();
    expect(el.style.getPropertyValue('--pd-toast-lift')).toBe('0px');
    TestBed.inject(ToastArea).setLift(48);
    await f.whenStable();
    expect(el.style.getPropertyValue('--pd-toast-lift')).toBe('calc(48px + var(--pd-space-2))');
    TestBed.inject(ToastArea).setLift(0);
    await f.whenStable();
    expect(el.style.getPropertyValue('--pd-toast-lift')).toBe('0px');
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
    // The newest comes first (see 'the stack does not move under the pointer').
    expect(all(el, '[data-testid=notification]').map((t) => [t.getAttribute('role'), t.getAttribute('data-severity')])).toEqual([
      ['status', 'info'],
      ['alert', 'error'],
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
    expect(one(el, '[data-cause=b] .pd-err').textContent).toContain('nope');
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
    // The host looks every 250 ms: its next look after the new ttl ran out (at 1900) is at 2000.
    clock.advance(200);
    expect(dismiss).toHaveBeenCalled();
  });

  it('holds the time while the pointer or focus is in the stack', async () => {
    const dismiss = vi.fn();
    const { f, list, clock, el } = setup([n('i', { dismiss, ttl: 1000 })]);
    await f.whenStable();
    const mouse = pointer(el, clock);
    mouse.enter();
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    // A repeat while held stays held.
    list.set([n('i', { dismiss, ttl: 1000, stamp: 'again' })]);
    await f.whenStable();
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    // Focus inside keeps it held after the pointer has gone.
    const button = one(el, '[data-testid=notification-dismiss]');
    focusIn(button);
    mouse.leave();
    clock.advance(5000);
    expect(dismiss).not.toHaveBeenCalled();
    focusOut(button);
    clock.advance(HOLD_RELEASE_MS);
    clock.advance(999);
    expect(dismiss).not.toHaveBeenCalled();
    // The host looks every 250 ms: its next look after the time ran out.
    clock.advance(250);
    expect(dismiss).toHaveBeenCalledTimes(1);
  });

  describe('the stack does not move under the pointer', () => {
    const order = (el: HTMLElement) => all(el, '[data-testid=notification]').map((t) => t.getAttribute('data-cause'));

    it('the newest toast enters at the top: the ones already there keep their elements', async () => {
      const { f, list, el } = setup([n('a')]);
      await f.whenStable();
      const before = one(el, '[data-cause=a]');
      list.set([n('a'), n('b')]);
      await f.whenStable();
      expect(order(el)).toEqual(['b', 'a']);
      expect(one(el, '[data-cause=a]')).toBe(before);
    });

    it('posting a cause again updates it where it is', async () => {
      const { f, list, el } = setup([n('a'), n('b')]);
      await f.whenStable();
      list.set([n('a', { count: 2 }), n('b')]);
      await f.whenStable();
      expect(order(el)).toEqual(['b', 'a']);
      expect(one(el, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×2');
    });

    it('while the pointer is in the stack, a newcomer shows on top, and nothing is dropped or re-ordered until it leaves', async () => {
      const { f, list, clock, el } = setup([n('a'), n('b'), n('c')]);
      await f.whenStable();
      const mouse = pointer(el, clock);
      mouse.enter();
      list.set([n('a'), n('b'), n('c'), n('d'), n('a', { count: 5 })]);
      await f.whenStable();
      expect(order(el)).toEqual(['d', 'c', 'b', 'a']); // past the limit of three, but the ones on screen stay
      expect(one(el, '[data-cause=a] [data-testid=notification-count]').textContent).toContain('×5'); // updated in place
      mouse.leave();
      await f.whenStable();
      expect(order(el)).toEqual(['d', 'c', 'b']);
    });

    it('a new persistent toast shows while the stack is held', async () => {
      const { f, list, clock, el } = setup([n('a'), n('b')]);
      await f.whenStable();
      pointer(el, clock).enter();
      list.set([n('a'), n('b'), n('boom', { severity: 'critical' })]);
      await f.whenStable();
      expect(order(el)).toEqual(['boom', 'b', 'a']);
      expect(one(el, '[data-cause=boom]').getAttribute('role')).toBe('alert');
    });

    it('while the pointer is in the stack, a toast that goes away still goes away at once', async () => {
      const { f, list, clock, el } = setup([n('a'), n('b')]);
      await f.whenStable();
      pointer(el, clock).enter();
      list.set([n('a'), n('b'), n('c')]);
      await f.whenStable();
      list.set([n('a'), n('c')]);
      await f.whenStable();
      expect(order(el)).toEqual(['c', 'a']);
    });

    it('focus inside a toast that is removed releases the hold', async () => {
      const { f, list, el } = setup([n('a'), n('b'), n('c')]);
      await f.whenStable();
      focusIn(one(el, '[data-cause=a] [data-testid=notification-dismiss]'));
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await f.whenStable();
      expect(order(el)).toEqual(['d', 'c', 'b', 'a']); // held: nothing dropped
      list.set([n('b'), n('c'), n('d'), n('e')]); // a goes, and with it the focus
      await f.whenStable();
      await f.whenStable();
      expect(order(el)).toEqual(['e', 'd', 'c']); // released: the limit applies again
    });

    it('focus leaving the stack without a pointer event releases the hold', async () => {
      const { f, list, clock, el } = setup([n('a'), n('b'), n('c')]);
      await f.whenStable();
      const button = one(el, '[data-cause=b] [data-testid=notification-dismiss]');
      focusIn(button);
      focusOut(button);
      clock.advance(HOLD_RELEASE_MS);
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await f.whenStable();
      expect(order(el)).toEqual(['d', 'c', 'b']);
    });

    it('moving from one toast to the next across the gap does not release the hold', async () => {
      const { f, list, clock, el } = setup([n('a'), n('b'), n('c')]);
      await f.whenStable();
      const mouse = pointer(el, clock);
      mouse.enter();
      const stack = one(el, '[data-testid=notification-stack]');
      stack.dispatchEvent(new Event('pointerout', { bubbles: true })); // leaves a toast into the gap
      clock.advance(HOLD_RELEASE_MS - 1);
      mouse.enter(); // and enters the next one
      clock.advance(HOLD_RELEASE_MS);
      list.set([n('a'), n('b'), n('c'), n('d')]);
      await f.whenStable();
      expect(order(el)).toEqual(['d', 'c', 'b', 'a']);
    });

    it('the pointer on the stack holds the time of every toast in it', async () => {
      const gone: string[] = [];
      const { f, clock, el } = setup([n('a', { dismiss: () => gone.push('a') }), n('b', { dismiss: () => gone.push('b') })]);
      await f.whenStable();
      const mouse = pointer(el, clock);
      mouse.enter();
      clock.advance(TOAST_MS * 5);
      expect(gone).toEqual([]);
      mouse.leave();
      clock.advance(TOAST_MS + 250); // the host looks every 250 ms
      expect(gone.sort()).toEqual(['a', 'b']);
    });
  });

  it('forgets the timer of a toast that went', async () => {
    const dismiss = vi.fn();
    const { f, list, clock } = setup([n('i', { dismiss }), n('j', { severity: 'error' })]);
    await f.whenStable();
    list.set([n('j', { severity: 'error' })]);
    await f.whenStable();
    clock.advance(TOAST_MS * 2);
    expect(dismiss).not.toHaveBeenCalled();
    expect(clock.ticking).toBe(0);
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

describe('pd-notification-host in a small window (review 2, P3-14)', () => {
  // jsdom does no layout, so the rules are read from the component's own styles; e2e/shell-demo.spec.ts measures them in a browser.
  // Angular keeps the styles scoped and pretty-printed (`.a[_ngcontent-%COMP%] {\n  width: 14px;`): flatten them to `.a { width: 14px; }`.
  const css = ((PdNotificationHost as unknown as { ɵcmp: { styles: string[] } }).ɵcmp.styles ?? [])
    .join('\n')
    .replace(/\[_ng(?:content|host)-%COMP%\]/g, '')
    .replace(/\s+/g, ' ');
  const compact = /@media \(max-width: 480px\) \{(.*?)\} \}/.exec(css)?.[1] ?? '';

  it('keeps the page-footer safe area above the status bar, from tokens, so a toast never covers a footer action', () => {
    expect(css).toMatch(/\bbottom: calc\(var\(--(?:%NS%)?pd-statusbar-height[^)]*\) \+ var\(--(?:%NS%)?pd-page-footer-safe[^)]*\) \+ var\(--(?:%NS%)?pd-space-2\)\)/);
  });

  it('the stack never takes clicks itself: only the toasts do, so the page beside them stays reachable (also at 480 px and below)', () => {
    expect(css).toMatch(/^\s*\{ position: fixed;[^}]*pointer-events: none;/);
    expect(css).toMatch(/\.pd-toast \{ pointer-events: auto;/);
    expect(css).not.toMatch(/\.pd-stack \{[^}]*pointer-events: auto/);
    expect(compact).not.toMatch(/pointer-events: auto/);
  });

  it('a toast is never wider than 360 px', () => {
    expect(/\.pd-toast \{[^}]*\bwidth: 360px;/.test(css)).toBe(true);
    expect(css).not.toMatch(/\bwidth: 3[7-9]\dpx/);
    expect(compact).toMatch(/max-width: 360px/);
  });

  it('at 480 px and below it is one line: the message, the first action and the close, above the status bar', () => {
    expect(compact).not.toBe('');
    expect(compact).toMatch(/\.pd-toast \{[^}]*flex-direction: row/);
    expect(compact).toMatch(/\.pd-title \{[^}]*white-space: nowrap/);
    expect(compact).toMatch(/\.pd-actions \.pd-btn:not\(:first-child\) \{ display: none;/);
    expect(compact).toMatch(/\.pd-body:not\(\.pd-err\)/);
    // The older toasts wait in the centre: two show at most.
    expect(compact).toMatch(/\.pd-toast:nth-child\(n ?\+ ?3\) \{ display: none;/);
    // The close stays reachable and in the flow.
    expect(compact).toMatch(/\.pd-x \{[^}]*position: static/);
  });

  it('shows a notice\'s short form instead of its title at 480 px and below, and the title otherwise (review 3 P3-8)', () => {
    expect(css).toMatch(/\.pd-short \{ display: none; \}/);
    expect(compact).toMatch(/\.pd-has-short \.pd-full \{ display: none; \}/);
    expect(compact).toMatch(/\.pd-has-short \.pd-short \{ display: inline; \}/);
  });

  it('keeps every action and the close in the markup (the small form only hides them), the primary first as authored', async () => {
    const list = signal<readonly HostNotification[]>([
      n('a', { actions: [{ label: 'Undo', kind: 'primary', run: () => undefined }, { label: 'More', kind: 'ghost', run: () => undefined }], body: 'long body', details: 'd' }),
    ]);
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: NOTIFY_CLOCK, useValue: new FakeClock() }, { provide: NOTIFICATION_SOURCES, multi: true, useValue: { items: list } }],
    });
    const fixture = TestBed.createComponent(PdNotificationHost);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect([...el.querySelectorAll('.pd-actions .pd-btn')].map((b) => b.textContent?.trim())).toEqual(['Undo', 'More', 'Details']);
    expect(el.querySelector('[data-testid=notification-dismiss]')).not.toBeNull();
  });
});
