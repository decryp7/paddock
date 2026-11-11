import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, signal, untracked } from '@angular/core';
import { Icon } from '../shared/icon';
import { HOLD_RELEASE_MS, HostAction, HostNotification, MAX_VISIBLE, mergeNotifications, NOTIFICATION_SOURCES, persists, TOAST_MS } from './notification-host.model';

/**
 * The one place notifications appear (ADR 0055), replacing a separate toast component per feature. One toast per cause,
 * details collapsed behind "Details", updated in place; errors and critical failures stay until resolved or dismissed;
 * a cause the status chip states (`health:...`) is never toasted. The host owns the time of every toast (sources only post, update
 * and resolve): a toast that times out stays `TOAST_MS`, and starts over when its cause is posted again. Mounted once per window.
 *
 * Nothing moves under the pointer. The stack is anchored at the bottom of the window, so the newest toast enters at the top (the
 * DOM lists the newest first, as they appear) and the ones already there keep their place; a cause posted again is updated in
 * place. While the pointer or focus is on a toast, the stack's time is held and what is on screen is frozen: nothing is dropped
 * past `MAX_VISIBLE` and nothing is re-ordered, a newcomer (a persistent one too) shows on top, and only what goes away (dismissed,
 * resolved, muted) leaves at once. A press is two events, and a layout change between them would send the click to another
 * element. The hold follows the DOM's own answer (focus inside, `:hover`), read after each change and a short grace after the
 * pointer or focus leaves, so a missed event can never keep the stack held. The stack itself takes no clicks (only the toasts
 * do), so the page beside and between them stays reachable.
 */
@Component({
  selector: 'debrief-notification-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { 'data-snapshot-ignore': '' },
  template: `
    <div class="stack" data-testid="notification-stack" role="region" aria-label="Notifications" i18n-aria-label="@@health.host.region" aria-live="polite"
         (pointerover)="enter()" (focusin)="enter()" (pointerout)="leaving()" (focusout)="leaving()">
      @for (n of items(); track n.cause) {
        <div
          class="toast"
          data-debrief-overlay
          data-toast
          [attr.data-severity]="n.severity"
          [attr.data-cause]="n.cause"
          data-testid="notification"
          [attr.role]="persistent(n) ? 'alert' : 'status'"
        >
          <div class="head">
            <debrief-icon [name]="icon(n)" [size]="15" />
            <strong class="title">{{ n.title }}@if ((n.count ?? 1) > 1) {<span class="count" data-testid="notification-count"> (×{{ n.count }})</span>}</strong>
          </div>
          @if (n.body) {
            <div class="body debrief-selectable">{{ n.body }}</div>
          }
          @if (n.actions?.length || n.details) {
            <div class="actions">
              @for (a of n.actions; track $index) {
                <button type="button" class="btn" [class.primary]="a.kind === 'primary'" [class.ghost]="a.kind === 'ghost'" [attr.data-testid]="a.id ? 'notification-action-' + a.id : 'notification-action'" (click)="run(n, a)">{{ a.label }}</button>
              }
              @if (n.details) {
                <button type="button" class="btn ghost" [attr.aria-expanded]="isOpen(n)" data-testid="notification-details-toggle" (click)="toggle(n)" i18n="@@health.host.details">Details</button>
              }
            </div>
          }
          @if (n.details && isOpen(n)) {
            <code class="details debrief-selectable" data-testid="notification-details">{{ n.details }}</code>
          }
          @if (failed()[n.cause]; as why) {
            <div class="body err" role="alert">{{ why }}</div>
          }
          <button type="button" class="x" aria-label="Dismiss notification" i18n-aria-label="@@health.host.dismiss" data-testid="notification-dismiss" (click)="n.dismiss()"><debrief-icon name="close" [size]="12" /></button>
        </div>
      }
    </div>
  `,
  styles: `
    :host { position: fixed; right: 16px; bottom: 32px; z-index: 1900; pointer-events: none; }
    .stack { display: flex; flex-direction: column; gap: 8px; align-items: flex-end; }
    .toast { pointer-events: auto; position: relative; width: 380px; max-width: calc(100vw - 32px); box-sizing: border-box; padding: 10px 32px 10px 12px; border: 1px solid var(--border-2); border-left: 3px solid var(--accent); border-radius: 8px; background: var(--bg-3); color: var(--text); box-shadow: var(--shadow); display: flex; flex-direction: column; gap: 6px; font-size: var(--fs-md); }
    .toast[data-severity='success'] { border-left-color: var(--ok); }
    .toast[data-severity='warning'] { border-left-color: var(--lvl-warn); }
    .toast[data-severity='error'] { border-left-color: var(--lvl-error); }
    .toast[data-severity='critical'] { border-left-color: var(--lvl-fatal); }
    .head { display: flex; align-items: center; gap: 8px; min-width: 0; }
    .title { overflow-wrap: anywhere; }
    .count { color: var(--muted); font-weight: 400; font-size: var(--fs-xs); }
    .body { color: var(--text-2); line-height: 1.4; overflow-wrap: anywhere; }
    .err { color: var(--danger-text, var(--lvl-error)); }
    .actions { display: flex; gap: 6px; flex-wrap: wrap; }
    .details { display: block; max-height: 160px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--muted); font-size: var(--fs-xs); }
    .x { all: unset; position: absolute; top: 6px; right: 8px; cursor: pointer; color: var(--text-2); line-height: 1; }
    .x:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  `,
})
export class NotificationHost {
  private readonly sources = inject(NOTIFICATION_SOURCES, { optional: true }) ?? [];
  /** Every toast, oldest first (not yet cut to `MAX_VISIBLE`). */
  private readonly merged = computed(() => mergeNotifications(this.sources.flatMap((s) => s.items())));
  /** The causes on screen when the pointer or focus came into the stack, oldest first; null while it is outside. */
  private readonly frozen = signal<readonly string[] | null>(null);
  /**
   * What is shown, newest first. Free: the newest `MAX_VISIBLE`. Held: what was on screen (still posted, updated in place, in
   * its order) with every newcomer on top; only the drop past the limit and the re-order wait for the release.
   */
  protected readonly items = computed(() => {
    const merged = this.merged();
    const frozen = this.frozen();
    if (frozen === null) return merged.slice(-MAX_VISIBLE).reverse();
    const kept = frozen.map((c) => merged.find((n) => n.cause === c)).filter((n): n is HostNotification => !!n);
    const newcomers = merged.filter((n) => !frozen.includes(n.cause));
    return [...kept, ...newcomers].reverse();
  });
  private readonly opened = signal<ReadonlySet<string>>(new Set());
  /** An action that rejected: why, until the next try. */
  protected readonly failed = signal<Record<string, string>>({});

  protected persistent(n: HostNotification): boolean {
    return persists(n);
  }

  /** Per cause: the occurrence the running timer belongs to, and the timer. */
  private readonly timers = new Map<string, { key: string; handle: ReturnType<typeof setTimeout> | undefined }>();
  /** The pointer or focus is in the stack: no toast's time runs. */
  private held = false;
  /** The pending release check, when one is waiting. */
  private releaseTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly injector = inject(Injector);
  private readonly element = inject(ElementRef<HTMLElement>).nativeElement as HTMLElement;

  constructor() {
    // Timers are outside the signal graph: one per toast that times out, started when it appears or is posted again.
    effect(() => {
      this.sync(this.items());
      // Whatever changed, a hold nothing keeps up (the focused toast went, the pointer was left without an event) ends: checked
      // once the change is on screen, because the focused toast is only gone from the DOM then.
      untracked(() => afterNextRender(() => this.releaseIfFree(), { injector: this.injector }));
    });
    inject(DestroyRef).onDestroy(() => {
      this.timers.forEach((t) => clearTimeout(t.handle));
      clearTimeout(this.releaseTimer);
    });
  }

  private sync(items: readonly HostNotification[]): void {
    const live = new Set(items.map((n) => n.cause));
    for (const [cause, t] of this.timers) {
      if (live.has(cause)) continue;
      clearTimeout(t.handle);
      this.timers.delete(cause);
    }
    for (const n of items) {
      if (persists(n)) continue;
      const key = `${n.instance ?? ''}#${n.stamp ?? n.count ?? ''}`;
      const current = this.timers.get(n.cause);
      if (current?.key === key) continue;
      clearTimeout(current?.handle);
      this.timers.set(n.cause, { key, handle: this.held ? undefined : this.start(n) });
    }
  }

  private start(n: HostNotification): ReturnType<typeof setTimeout> {
    return setTimeout(() => n.dismiss(), n.ttl ?? TOAST_MS);
  }

  private stack(): HTMLElement | null {
    return this.element.querySelector<HTMLElement>('[data-testid=notification-stack]');
  }

  /** Whether the DOM says the pointer or focus is on one of the toasts. */
  private inside(): boolean {
    const stack = this.stack();
    return !!stack && (stack.contains(document.activeElement) || stack.matches(':hover'));
  }

  /** The pointer or focus came onto a toast: the time of every toast stops and what is on screen is frozen. */
  protected enter(): void {
    clearTimeout(this.releaseTimer);
    this.releaseTimer = undefined;
    if (this.held) return;
    this.held = true;
    this.frozen.set(this.items().map((n) => n.cause).reverse());
    for (const t of this.timers.values()) {
      clearTimeout(t.handle);
      t.handle = undefined;
    }
  }

  /** The pointer or focus left a toast: after a short grace (the gap to the next toast) the hold is checked. */
  protected leaving(): void {
    if (!this.held) return;
    clearTimeout(this.releaseTimer);
    this.releaseTimer = setTimeout(() => {
      this.releaseTimer = undefined;
      this.releaseIfFree();
    }, HOLD_RELEASE_MS);
  }

  /** Lets go of the stack when neither the pointer nor focus is on it: the time of every toast starts over, the waiting drops happen. */
  private releaseIfFree(): void {
    if (!this.held || this.inside()) return;
    this.held = false;
    this.frozen.set(null);
    this.sync(this.items());
    for (const [cause, t] of this.timers) {
      const n = this.items().find((x) => x.cause === cause);
      clearTimeout(t.handle);
      t.handle = n && !persists(n) ? this.start(n) : undefined;
    }
  }

  protected icon(n: HostNotification): string {
    return n.severity === 'success' ? 'check' : n.severity === 'info' ? 'bell' : 'alert';
  }

  private key(n: HostNotification): string {
    return `${n.cause}#${n.instance ?? ''}`;
  }

  protected isOpen(n: HostNotification): boolean {
    return this.opened().has(this.key(n));
  }

  protected toggle(n: HostNotification): void {
    const key = this.key(n);
    this.opened.update((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  /** The pointer or focus left the stack: once neither is in it, the toasts' time runs again and the waiting ones appear. */
  protected leave(e: MouseEvent | FocusEvent): void {
    const stack = e.currentTarget as HTMLElement;
    const focusStays = e instanceof FocusEvent ? stack.contains(e.relatedTarget as Node | null) : stack.contains(document.activeElement);
    const pointerStays = e instanceof FocusEvent && stack.matches(':hover');
    if (focusStays || pointerStays || !this.held) return;
    this.held = false;
    this.frozen.set(null);
    this.sync(this.items());
    for (const [cause, t] of this.timers) {
      const n = this.items().find((x) => x.cause === cause);
      clearTimeout(t.handle);
      t.handle = n && !persists(n) ? this.start(n) : undefined;
    }
  }

  /** Runs an action; the toast stays when it failed, with the reason. */
  protected async run(n: HostNotification, a: HostAction): Promise<void> {
    this.failed.update((f) => ({ ...f, [n.cause]: '' }));
    try {
      await a.run();
    } catch (e) {
      this.failed.update((f) => ({ ...f, [n.cause]: e instanceof Error ? e.message : String(e) }));
    }
  }
}
