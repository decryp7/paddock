import { effect, inject, Injectable, Injector, type Signal, untracked } from '@angular/core';
import { DockViews } from './dock-views';

/** What the policy drives: the dock, once it has a layout. */
export interface DockPolicyTarget {
  /** True once the dock has a layout to show panels in. */
  readonly ready: Signal<boolean>;
  syncConditionalViews(): void;
}

/**
 * The live `when` of views for the dock, kept out of the dock itself: a view whose `when` starts to hold and which asks
 * for `autoOpen` opens in the background; a view whose `when` stops holding is closed (`DockService.syncConditionalViews`).
 * The registry's `availableViews` reads the app's context, so the effect follows it.
 */
@Injectable()
export class DockPolicy {
  private readonly views = inject(DockViews);
  private readonly injector = inject(Injector);

  /** Starts the rules for `dock`; called once by the dock. */
  start(dock: DockPolicyTarget): void {
    // effect-state: opens and closes panels in dockview when the context changes (a side effect on an outside system)
    effect(
      () => {
        this.views.available();
        if (dock.ready()) untracked(() => dock.syncConditionalViews());
      },
      { injector: this.injector },
    );
  }
}
