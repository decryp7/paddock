import { DOCUMENT } from '@angular/common';
import { computed, DestroyRef, effect, inject, Injectable, untracked } from '@angular/core';
import { ShellUiStore } from '../core/workspace/shell-ui.store';
import { UiPrefsStore } from '../core/workspace/ui-prefs.store';
import { WorkspaceStore } from '../core/workspace.store';
import { DockService } from '../dock/dock.service';
import { explorerAction, fitExplorer } from './explorer-fit';

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
  private readonly document = inject(DOCUMENT);
  /** The sidebar was open when the window narrowed and was closed by this service. */
  private closedByNarrowing = false;
  private started = false;
  /** The user is dragging the Explorer's sash: it is not judged until the drag ends. */
  private held = false;

  /** A reset (or any change of the flag) is judged at once: the width and the dock's yield change with it, and the dock re-plans. */
  private readonly followChosen = effect(() => {
    this.chosen();
    untracked(() => {
      if (this.started) this.evaluate();
    });
  });

  /** Starts following the dock's re-plans (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    this.destroyRef.onDestroy(this.dock.onWindowPlan(() => this.evaluate()));
    this.evaluate();
  }

  /**
   * The user has set a width of their own (`UiPrefsStore.explorerWidthChosen`): an explicit resize wins over the other columns, which
   * the dock tabs; the default width yields to them instead (the Explorer shrinks so Registry or Home keep their column).
   */
  private readonly chosen = this.prefs.explorerWidthChosen;

  /** The Explorer's width and the room it lends the dock, from the user's wish and the rooms the plan found last. */
  private readonly fit = computed(() =>
    fitExplorer({
      open: this.store.sidebarOpen(),
      chosen: this.chosen(),
      wish: this.prefs.explorerWidth(),
      hardRoom: this.shell.explorerRoom(),
      columnRoom: this.shell.explorerColumnRoom(),
    }),
  );

  /** The width the Explorer is shown at, px: the user's, within the room it has now (all the window leaves the Events view when the user chose the width, else what leaves every column its own). */
  readonly width = computed(() => this.fit().shown);

  /** A drag of the Explorer's sash began: the plan is not asked about the Explorer until `release`. */
  hold(): void {
    this.held = true;
    // An explicit resize takes room from the other columns: the dock plans with the window it really has.
    this.dock.setExplorerYield(0);
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
    // The window the plan sees is the dock plus the Explorer as it is on screen now (the width to show may differ until it renders).
    const onScreen = open ? Math.round(this.document.getElementById('debrief-explorer')?.getBoundingClientRect().width || shown) : wish;
    const verdict = this.dock.explorerVerdict(open, onScreen);
    const hardRoom = open ? this.dock.explorerRoom(true, onScreen) : this.shell.explorerRoom();
    const columnRoom = open ? this.dock.explorerColumnRoom(true, onScreen) : this.shell.explorerColumnRoom();
    this.dock.setExplorerYield(this.fit().yield);
    if (open) {
      this.shell.setExplorerRoom(hardRoom);
      this.shell.setExplorerColumnRoom(columnRoom);
    }
    const next = explorerAction(open, verdict, this.closedByNarrowing);
    this.closedByNarrowing = next.closedByNarrowing;
    if (next.action !== 'none') this.store.toggleSidebar();
  }

  /**
   * Opens or closes the Explorer for the user (Ctrl+B, the activity bar, the View menu). Closing is always allowed. Opening is
   * judged before anything renders: an Explorer that cannot fit at its minimum beside the Events view at its minimum stays closed,
   * and a notice says to widen the window.
   */
  toggle(): void {
    if (this.store.sidebarOpen() || this.dock.explorerFits()) {
      this.store.toggleSidebar();
      return;
    }
    this.dock.notifyExplorerCannotFit();
  }

  /** Opens the Explorer if it is closed and fits (focus moves and menu commands that need it); true when it is open after. */
  ensureOpen(): boolean {
    if (!this.store.sidebarOpen()) this.toggle();
    return this.store.sidebarOpen();
  }
}
