import { computed, DestroyRef, inject, Injectable } from '@angular/core';
import { ShellUiStore } from '../core/workspace/shell-ui.store';
import { EXPLORER_WIDTH, UiPrefsStore } from '../core/workspace/ui-prefs.store';
import { WorkspaceStore } from '../core/workspace.store';
import { DockService } from '../dock/dock.service';

/**
 * Keeps the window usable when it is narrow, by the layout plan (ADR 0056) and not by window-width constants: the
 * Explorer is a view of the plan (a left column outside the dock, priority 80), so it closes when the plan has no room
 * for it beside what is docked, and comes back, with a margin so it cannot flap, when there is. Lower-priority tool
 * windows yield first (the dock tabs or auto-hides them in the same plan). Without room an Explorer opened by
 * hand (Ctrl+B) closes again at once (there is no overlay mode); one the user closed is never reopened. Its width is the
 * user's, kept while the window is narrow (it is shown within the room the plan has) and restored when it widens.
 */
@Injectable({ providedIn: 'root' })
export class NarrowWindow {
  private readonly store = inject(WorkspaceStore);
  private readonly dock = inject(DockService);
  private readonly prefs = inject(UiPrefsStore);
  private readonly shell = inject(ShellUiStore);
  private readonly destroyRef = inject(DestroyRef);
  /** The sidebar was open when the window narrowed and was closed by this service. */
  private closedByNarrowing = false;
  private started = false;
  /** The user is dragging the Explorer's sash: it is not judged until the drag ends. */
  private held = false;

  /** Starts following the dock's re-plans (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    this.destroyRef.onDestroy(this.dock.onWindowPlan(() => this.evaluate()));
    this.evaluate();
  }

  /** The width the Explorer is shown at, px: the user's, within the room the plan has now. */
  readonly width = computed(() => Math.min(this.prefs.explorerWidth(), this.shell.explorerRoom()));

  /** A drag of the Explorer's sash began: the plan is not asked about the Explorer until `release`. */
  hold(): void {
    this.held = true;
  }

  /** The sash is gone (the Explorer closed mid-drag): a drag that never ended must not leave the plan held. */
  releaseIfHeld(): void {
    if (this.held) this.release();
  }

  /** The drag ended (or a key press finished): the Explorer is judged once, now. */
  release(): void {
    this.held = false;
    this.evaluate();
  }

  /**
   * Asks the plan whether the Explorer fits and closes or reopens it accordingly; exposed for tests. The width it is asked
   * about is the user's (`UiPrefsStore.explorerWidth`), shown within the room the plan has: the plan shrinks the Explorer
   * towards its declared minimum before it closes it.
   */
  evaluate(): void {
    if (this.held) return;
    const open = this.store.sidebarOpen();
    const wish = this.prefs.explorerWidth();
    const shown = this.width();
    const verdict = this.dock.explorerVerdict(open, open ? shown : wish);
    this.dock.setExplorerYield(open ? shown - EXPLORER_WIDTH.min : 0);
    if (open) this.shell.setExplorerRoom(this.dock.explorerRoom(true, shown));
    if (open) {
      // No room beside the message column's minimum: the Explorer is closed again, also when the user has just opened it
      // (there is no overlay mode); it reopens at the user's width when the window is wide enough.
      if (verdict === 'hide') {
        this.closedByNarrowing = true;
        this.store.toggleSidebar();
      } else this.closedByNarrowing = false;
    } else if (verdict === 'show' && this.closedByNarrowing) {
      this.closedByNarrowing = false;
      this.store.toggleSidebar();
    }
  }
}
