import { DOCUMENT } from '@angular/common';
import { afterNextRender, DestroyRef, effect, inject, Injectable, Injector, untracked } from '@angular/core';
import { WorkspaceStore } from '../core/workspace.store';
import { EXPLORER_LAYOUT } from '../dock/dock-layout';
import { DockService } from '../dock/dock.service';

/**
 * Keeps the window usable when it is narrow, by the layout plan (ADR 0056) and not by window-width constants: the
 * Explorer is a view of the plan (a left column outside the dock, priority 80), so it closes when the plan has no room
 * for it beside what is docked, and comes back, with a margin so it cannot flap, when there is. Lower-priority tool
 * windows yield first (the dock tabs or auto-hides them in the same plan). The user's choice wins: an Explorer opened
 * by hand (Ctrl+B) stays open until the plan has room again; one the user closed is never reopened.
 */
@Injectable({ providedIn: 'root' })
export class NarrowWindow {
  private readonly store = inject(WorkspaceStore);
  private readonly dock = inject(DockService);
  private readonly doc = inject(DOCUMENT);
  private readonly destroyRef = inject(DestroyRef);
  private readonly injector = inject(Injector);
  /** The sidebar was open when the window narrowed and was closed by this service. */
  private closedByNarrowing = false;
  /** The user opened the Explorer by hand while there was no room: leave it open until it fits again. */
  private keptByUser = false;
  private started = false;

  /** Starts following the dock's re-plans (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    // The Explorer is first drawn when data arrives (under the welcome page it is hidden), together with the filter bar that takes part of the dock's
    // height: decide once, after the render that draws them and before the browser does (Angular renders again before it yields).
    // effect-state: a one-time layout decision after a render (a store toggle that must follow the DOM), not derived state
    const first = effect(
      () => {
        if ((this.store.summary()?.sources.length ?? 0) === 0) return;
        untracked(() => afterNextRender(() => this.decideAtStart(), { injector: this.injector }));
        first.destroy();
      },
      { injector: this.injector },
    );
    this.destroyRef.onDestroy(this.dock.onWindowPlan(() => this.evaluate()));
    this.evaluate();
  }

  /**
   * The Explorer's state when data first arrives, from the layout plan's own decision: the same `explorerVerdict` call as every later re-plan, so the two
   * cannot differ and there is no width constant of ours. Called after the render that first draws the Explorer together with the filter bar, before the
   * browser draws: the dock is laid out at its container's size now (`followContainer`) and the plan is asked with the Explorer open. When the plan has
   * no room, a narrow window starts without the Explorer and may get it back when the plan has room, as if this service had closed it; the render that
   * removes it also happens before the first draw, and the dock follows its container in the same frame (`DockLayout.followContainer`), so nothing is drawn
   * at the old size. Returns whether it closed the Explorer. The Explorer's open flag is not saved anywhere (only the sections' state is), so there is no
   * saved choice to honour.
   * Relies on Angular rendering again, in the same tick, when an `afterNextRender` hook changes state (the toggle is made in one); pinned by the test
   * `Angular renders again in the same tick ...` in narrow-window.spec.ts.
   */
  decideAtStart(): boolean {
    if (!this.store.sidebarOpen()) return false;
    this.dock.followContainer();
    if (this.dock.explorerVerdict(true, this.width()) !== 'hide') return false;
    this.closedByNarrowing = true;
    this.store.toggleSidebar();
    return true;
  }

  /** Asks the plan whether the Explorer fits and closes or reopens it accordingly; exposed for tests. */
  evaluate(): void {
    const open = this.store.sidebarOpen();
    const verdict = this.dock.explorerVerdict(open, this.width());
    if (open) {
      if (this.closedByNarrowing) {
        // It was closed by this service and is open again: the user opened it.
        this.closedByNarrowing = false;
        this.keptByUser = verdict === 'hide';
        return;
      }
      if (verdict === 'keep') this.keptByUser = false;
      else if (verdict === 'hide' && !this.keptByUser) {
        this.closedByNarrowing = true;
        this.store.toggleSidebar();
      }
    } else if (verdict === 'show' && this.closedByNarrowing) {
      this.closedByNarrowing = false;
      this.store.toggleSidebar();
    }
  }

  private width(): number {
    const w = this.doc.querySelector('debrief-sidebar')?.getBoundingClientRect().width ?? 0;
    return w > 0 ? w : EXPLORER_LAYOUT.pref.width;
  }
}
