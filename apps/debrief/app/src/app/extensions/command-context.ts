import { computed, effect, EffectRef, inject, Injectable, Injector, Signal, untracked } from '@angular/core';
import { DISMISSED, ShellContextHost, type ShellCore, type ShellNotice, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { FieldFilter, Query, RecordView, WorkspaceSummary } from '../core/api.types';
import { WorkspaceStore } from '../core/workspace.store';
import { DockService } from '../dock/dock.service';
import { Perspectives } from '../dock/perspectives';
import { GridSelection } from '../grid/grid-selection';
import { WelcomeService } from '../welcome/welcome';
import type { CommandContext, DebriefContext, EventId, TimeDisplay, TimeRange, WorkspaceHandle } from './context';

/** How long `jumpTo` waits for the grid (it may not be open) before resolving anyway. */
export const JUMP_TIMEOUT_MS = 5000;

/** The main window's workspace; pop-out windows have their own (debrief-1-2-api.md §14.3). */
export const MAIN_WORKSPACE: WorkspaceHandle = Object.freeze({ window: 'main' });

/** Debrief's part of the context: the documented, narrow way for contributions to read and act on the window. */
@Injectable({ providedIn: 'root' })
export class WorkspaceCommandContext implements DebriefContext {
  private readonly store = inject(WorkspaceStore);
  private readonly dock = inject(DockService);
  private readonly welcome = inject(WelcomeService);
  private readonly injector = inject(Injector);
  private readonly perspectives = inject(Perspectives);
  private readonly grid = inject(GridSelection);

  readonly workspace = MAIN_WORKSPACE;
  readonly selection: Signal<readonly EventId[]> = computed(() => this.grid.recordsOr(this.store.selected()).map((r) => r.id), { equal: sameIds });
  readonly timeRange: Signal<TimeRange | null> = computed(
    () => {
      const { from, to } = this.store.query();
      return from != null && to != null ? { from, to } : null;
    },
    { equal: (a, b) => a === b || (!!a && !!b && a.from === b.from && a.to === b.to) },
  );
  readonly timeDisplay: Signal<TimeDisplay> = computed(() => this.store.timezone());

  get summary(): WorkspaceSummary | null {
    return this.store.summary();
  }
  openPaths(paths: string[]): Promise<void> {
    return this.store.openPaths(paths);
  }
  setQuery(patch: Partial<Query>): void {
    this.store.patchQuery(patch);
  }
  addFilter(filter: FieldFilter): void {
    this.store.addFilter(filter);
  }
  selectRecord(record: RecordView | null): void {
    this.store.select(record);
  }
  jumpTo(id: EventId): Promise<void> {
    // The store's anchor makes the dock reveal the grid, which loads the page and selects the row.
    this.store.jumpTo(id);
    return new Promise((resolve) => {
      let ref: EffectRef | undefined;
      let settled = false;
      const done = () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        ref?.destroy();
        resolve();
      };
      const timer = setTimeout(done, JUMP_TIMEOUT_MS);
      ref = effect(
        () => {
          if (this.store.selected()?.id === id || this.store.anchorMissed()) untracked(done);
        },
        { injector: this.injector, manualCleanup: true },
      );
      if (settled) ref.destroy();
    });
  }
  showView(id: string): void {
    // The welcome screen covers the dock while nothing is open: a view the user asks for must be visible.
    this.welcome.stepAside();
    this.dock.show(id);
  }
  suggestPerspective(id: string): void {
    this.perspectives.suggest(id);
  }
  notifyError(message: string): void {
    this.store.setError(message);
  }
}

/**
 * The window's `ShellContext<DebriefContext>`: the shell's part (`runCommand`, `notify`, ...) from `ShellContextHost`,
 * Debrief's from `WorkspaceCommandContext`. Built synchronously: Debrief registers no lazy app context in M1, so the
 * host's context is the shell core from the start. The shell's methods win over a Debrief member of the same name.
 */
export function debriefCommandContext(): CommandContext {
  const core = inject(ShellContextHost).context() as ShellCore;
  const app = inject(WorkspaceCommandContext);
  return combineContext(core, app);
}

/** `core` and `app` as one read-only object; getters of `app` keep their own `this`. Pure. */
export function combineContext<T extends object>(core: ShellCore, app: T): ShellCore & T {
  return new Proxy(app, {
    get: (target, prop) => (prop in core ? core[prop as keyof ShellCore] : Reflect.get(target, prop, target)),
    has: (target, prop) => prop in core || prop in target,
    set: () => false,
    defineProperty: () => false,
    deleteProperty: () => false,
  }) as ShellCore & T;
}

/**
 * The shell ports in Debrief's main window: notices go to Debrief's own notice and error banner until M2 moves
 * notifications to the shell; pages and dialogs are not the shell's yet (M2b, M3) and say so.
 */
export function debriefShellPorts(): ShellPorts {
  const store = inject(WorkspaceStore);
  return {
    showPage: (id) => console.warn(`[debrief] the shell's pages are not in Debrief yet (M3); cannot show page ${id}`),
    openDialog: (id) => {
      console.warn(`[debrief] the shell's dialogs are not in Debrief yet (M2b); cannot open dialog ${id}`);
      return Promise.resolve(DISMISSED);
    },
    notify: (n: ShellNotice) => (n.level === 'error' ? store.setError(n.message) : store.setNotice(n.message)),
  };
}

function sameIds(a: readonly EventId[], b: readonly EventId[]): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}
