import { computed, effect, inject, Injectable, Injector, signal, untracked } from '@angular/core';
import { Logger } from '../core/logger';
import { WorkspaceStore } from '../core/workspace.store';
import { DockService } from '../dock/dock.service';
import { NavHistory, NavPlace } from './nav-history';

/** After going back or forward, changes seen for this long (ms) are its echo (selection, panels sliding) and not recorded. */
export const NAV_ECHO_MS = 600;

/**
 * Back and forward through where the user has been (Alt+Left / Alt+Right, the mouse's back and forward buttons):
 * events they selected, time ranges they zoomed to, views they brought to the front. The places are recorded by
 * watching the workspace, so every control that moves them counts; going back or forward re-applies a place and the
 * watcher then sees it as the current one and records nothing.
 */
@Injectable({ providedIn: 'root' })
export class NavigationHistory {
  private readonly store = inject(WorkspaceStore);
  private readonly dock = inject(DockService);
  private readonly injector = inject(Injector);
  private readonly logger = inject(Logger);
  private readonly history = new NavHistory();
  private readonly version = signal(0);
  private started = false;
  /** Places seen before this time (ms) are the echo of a back or forward, not something the user did. */
  private quietUntil = 0;
  /** Clock, replaceable in tests. */
  now: () => number = () => Date.now();

  /** A view that is already in front is not a stop: going there would change nothing, and the key would seem to do nothing. */
  private readonly noChange = (place: NavPlace): boolean => place.kind === 'view' && place.id === this.dock.activeViewId();

  readonly canBack = computed(() => (this.version(), this.history.canGo(-1, this.noChange)));
  readonly canForward = computed(() => (this.version(), this.history.canGo(1, this.noChange)));

  /** Starts watching (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    effect(
      () => {
        const id = this.store.selected()?.id;
        if (id !== undefined) untracked(() => this.observe({ kind: 'event', id }));
      },
      { injector: this.injector },
    );
    // The range alone, compared by value: any other change of the query (a filter, a search, a jump's own change) is not a place. Read from the
    // whole query, it recorded the unchanged range as a new place, which dropped the places ahead whenever that change landed after the echo
    // window (debrief.yml run 8636: Alt+Right went nowhere on a loaded runner).
    const range = computed(
      () => {
        const q = this.store.query();
        return q.window ? null : { from: q.from ?? null, to: q.to ?? null };
      },
      { equal: (a, b) => a === b || (a !== null && b !== null && a.from === b.from && a.to === b.to) },
    );
    effect(
      () => {
        const r = range();
        if (r) untracked(() => this.observe({ kind: 'range', ...r }));
      },
      { injector: this.injector },
    );
    effect(
      () => {
        const id = this.dock.activeViewId();
        // Tool windows (the inspector sliding out on a selection) come and go by themselves: they are not places.
        if (id && untracked(() => this.dock.kindOf?.(id)) !== 'tool') untracked(() => this.observe({ kind: 'view', id }));
      },
      { injector: this.injector },
    );
  }

  /** @internal Records a place (exposed for tests). */
  observe(place: NavPlace): void {
    if (this.now() < this.quietUntil) {
      this.logger.debug('ui.nav', 'Echo ignored', describe(place));
      return;
    }
    if (this.history.record(place, this.now())) {
      this.version.update((v) => v + 1);
      this.logger.debug('ui.nav', 'Place recorded', { ...describe(place), Places: this.history.size });
    }
  }

  back(): boolean {
    return this.go(this.history.back(this.noChange));
  }

  forward(): boolean {
    return this.go(this.history.forward(this.noChange));
  }

  /** Forgets every place (another project or workspace replaces what they referred to). */
  clear(): void {
    this.history.clear();
    this.version.update((v) => v + 1);
  }

  private go(place: NavPlace | null): boolean {
    this.version.update((v) => v + 1);
    if (!place) return false;
    this.logger.debug('ui.nav', 'Went to a place', describe(place));
    this.quietUntil = this.now() + NAV_ECHO_MS;
    switch (place.kind) {
      case 'event':
        this.store.jumpTo(place.id);
        break;
      case 'range':
        this.store.patchQuery({ from: place.from, to: place.to });
        break;
      case 'view':
        this.dock.show(place.id);
        break;
    }
    return true;
  }
}

/** A place for the log: its kind and its id or range (no user content). */
function describe(place: NavPlace): Record<string, string | number | null> {
  if (place.kind === 'range') return { Kind: 'range', From: place.from, To: place.to };
  return { Kind: place.kind, Id: place.id };
}
