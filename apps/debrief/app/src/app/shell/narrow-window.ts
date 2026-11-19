import { DOCUMENT } from '@angular/common';
import { DestroyRef, inject, Injectable } from '@angular/core';
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
/**
 * Windows narrower than this start with the Explorer closed. A hand copy of a decision the layout plan makes (an open finding, docs/lessons.md; the plan
 * cannot be asked before the first draw because its input includes the dock's height, which the filter bar of the first data takes a part of):
 * measured 2026-11-17 (Chromium), the plan keeps the Explorer from 936 px at 600 to 900 px high and closes it below that: at 904 to 912 px (this constant
 * starts such a window without it, so its first draw is final), and at 920 to 934 px only after the first draw, so the dock's left edge moves 320 to
 * 48 there (fixed on fix/debrief-dock-same-frame). The plan stays the
 * authority after the first draw. The Explorer's open flag is not saved anywhere (only the sections' state is), so there is no saved choice to honour.
 */
export const EXPLORER_MIN_WINDOW_PX = 920;

@Injectable({ providedIn: 'root' })
export class NarrowWindow {
  private readonly store = inject(WorkspaceStore);
  private readonly dock = inject(DockService);
  private readonly doc = inject(DOCUMENT);
  private readonly destroyRef = inject(DestroyRef);
  /** The sidebar was open when the window narrowed and was closed by this service. */
  private closedByNarrowing = false;
  /** The user opened the Explorer by hand while there was no room: leave it open until it fits again. */
  private keptByUser = false;
  private started = false;

  /** Starts following the dock's re-plans (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    // Before the first draw: a narrow window starts without the Explorer (and may bring it back when the plan has room, as if it had closed it).
    const width = this.doc.defaultView?.innerWidth ?? Infinity;
    if (width < EXPLORER_MIN_WINDOW_PX && this.store.sidebarOpen()) {
      this.closedByNarrowing = true;
      this.store.toggleSidebar();
    }
    this.destroyRef.onDestroy(this.dock.onWindowPlan(() => this.evaluate()));
    this.evaluate();
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
