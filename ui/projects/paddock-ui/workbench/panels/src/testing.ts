import type { SyncTimers } from './ui-sync';
import { ChangeDetectionStrategy, Component, inject, InjectionToken, signal, type WritableSignal } from '@angular/core';
import { defineSharedState, defineView, provideWorkbenchContributions, VIEW_PANEL, type PanelRef } from '@decryp7/paddock-ui/workbench';
import { defineCommand, msg } from '@decryp7/paddock-ui/shell';
import { provideShellTesting } from '@decryp7/paddock-ui/shell/testing';

/** `SyncTimers` moved by hand (rule: no sleeps in tests): `advance(ms)` runs what fell due. */
export class FakeSyncTimers implements SyncTimers {
  private now = 0;
  private n = 0;
  private readonly timeouts = new Map<number, { at: number; run: () => void }>();
  private readonly intervals = new Map<number, { every: number; next: number; run: () => void }>();

  setTimeout(run: () => void, ms: number): unknown {
    const id = ++this.n;
    this.timeouts.set(id, { at: this.now + ms, run });
    return id;
  }

  clearTimeout(handle: unknown): void {
    this.timeouts.delete(handle as number);
  }

  setInterval(run: () => void, ms: number): unknown {
    const id = ++this.n;
    this.intervals.set(id, { every: ms, next: this.now + ms, run });
    return id;
  }

  clearInterval(handle: unknown): void {
    this.intervals.delete(handle as number);
  }

  /** Pending timers (timeouts and intervals). */
  get pending(): number {
    return this.timeouts.size + this.intervals.size;
  }

  advance(ms: number): void {
    const end = this.now + ms;
    for (;;) {
      const due = [
        ...[...this.timeouts].map(([id, t]) => ({ id, at: t.at, kind: 'timeout' as const })),
        ...[...this.intervals].map(([id, t]) => ({ id, at: t.next, kind: 'interval' as const })),
      ]
        .filter((d) => d.at <= end)
        .sort((a, b) => a.at - b.at)[0];
      if (!due) break;
      this.now = due.at;
      if (due.kind === 'timeout') {
        const t = this.timeouts.get(due.id);
        this.timeouts.delete(due.id);
        t?.run();
      } else {
        const t = this.intervals.get(due.id);
        if (t) {
          t.next += t.every;
          t.run();
        }
      }
    }
    this.now = end;
  }
}


/** The state a test window's shared-state definitions read and write. */
export interface TestCtx {
  readonly query: WritableSignal<string>;
  readonly theme: WritableSignal<string>;
  /** Which `VIEW_PANEL` the view component saw when it was created. */
  readonly seen: PanelRef[];
  /** The commands that ran. */
  readonly ran: string[];
}

/** A view component that records the panel it is rendered in. */
@Component({ changeDetection: ChangeDetectionStrategy.OnPush, selector: 'pd-test-view', template: `<p data-testid="test-view">{{ panel.panel }}</p>` })
export class TestView {
  protected readonly panel = inject(VIEW_PANEL);
  constructor() {
    inject(TEST_CTX).seen.push(this.panel);
  }
}

export const TEST_CTX = new InjectionToken<TestCtx>('TEST_CTX');

const layout = { area: 'document' as const, min: { width: 200, height: 100 }, pref: { width: 600, height: 400 }, priority: 50 };

/** The providers of a test window with two views (`wt.code` with instances, `wt.log`), a hidden view and two shared states. */
export function windowProviders(ctx: TestCtx) {
  return provideShellTesting<TestCtx>({
    context: () => Promise.resolve(() => ctx),
    providers: [
      { provide: TEST_CTX, useValue: ctx },
      provideWorkbenchContributions<TestCtx>({
        commands: [
          defineCommand<TestCtx>({ id: 'wt.undo', title: msg('wt.undo', 'Undo'), run: (c) => void c.ran.push('wt.undo') }),
          defineCommand<TestCtx>({ id: 'wt.format-disk', title: msg('wt.format-disk', 'Format'), run: (c) => void c.ran.push('wt.format-disk') }),
        ],
        views: [
          defineView<TestCtx>({ id: 'wt.code', title: msg('wt.code', 'Code'), icon: 'wt:code', layout, instances: true, load: () => Promise.resolve(TestView) }),
          defineView<TestCtx>({ id: 'wt.log', title: msg('wt.log', 'Log'), icon: 'wt:log', layout, load: () => Promise.resolve(TestView) }),
          defineView<TestCtx>({ id: 'wt.hidden', title: msg('wt.hidden', 'Hidden'), icon: 'wt:hidden', layout, load: () => Promise.resolve(TestView), when: () => false }),
        ],
        sharedState: [
          defineSharedState<TestCtx, string>({ id: 'wt.query', scope: 'full', read: (c) => c.query(), parse: (r) => (typeof r === 'string' ? r : undefined), apply: (v, c) => c.query.set(v) }),
          defineSharedState<TestCtx, string>({ id: 'wt.theme', scope: 'display', read: (c) => c.theme(), parse: (r) => (typeof r === 'string' ? r : undefined), apply: (v, c) => c.theme.set(v) }),
        ],
      }),
    ],
  });
}

export function newCtx(): TestCtx {
  return { query: signal(''), theme: signal('light'), seen: [], ran: [] };
}

import type { ChromeKind, ResizeDirection, WindowChromeIo } from './chrome';
import type { Unlisten } from './io';

/** A `WindowChromeIo` that records what the title bar asked and answers by hand. */
export class FakeChrome implements WindowChromeIo {
  readonly calls: string[] = [];
  maximized = false;
  dragged = true;
  resizeHandlers = new Set<() => void>();

  constructor(
    readonly kind: ChromeKind = 'frameless',
    readonly supportsKeepOnTop = true,
  ) {}

  async minimize(): Promise<void> {
    this.calls.push('minimize');
  }
  async toggleMaximize(): Promise<void> {
    this.maximized = !this.maximized;
    this.calls.push('toggleMaximize');
  }
  async close(): Promise<void> {
    this.calls.push('close');
  }
  async isMaximized(): Promise<boolean> {
    return this.maximized;
  }
  async onResized(handler: () => void): Promise<Unlisten> {
    this.resizeHandlers.add(handler);
    return () => this.resizeHandlers.delete(handler);
  }
  async startDragging(): Promise<boolean> {
    this.calls.push('startDragging');
    return this.dragged;
  }
  async startResize(direction: ResizeDirection): Promise<void> {
    this.calls.push(`resize:${direction}`);
  }
}
