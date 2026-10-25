import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject, Injector, signal, untracked } from '@angular/core';
import { PdPartOutlet } from '@decryp7/paddock-ui/shell';
import { panelTitle, VIEW_PANEL, WorkbenchRegistry, type ViewDef } from '@decryp7/paddock-ui/workbench';
import { WINDOW_CHROME } from './chrome';
import { PANELS_IO } from './io';
import { HOST_TITLE_KEY, hostTitleValue } from './panel-windows';
import { PdPopoutTitleBar } from './popout-title-bar';
import { POPOUT_ROUTE } from './popout-route';
import { PopoutState } from './popout-state';
import { SYNC_TIMERS, syncedValues, UiSync } from './ui-sync';
import { windowTitle } from './window-title';

/** A pop-out starts showing its view once it caught up with the host, or after this long anyway (the host may be gone or busy). */
export const SYNC_START_TIMEOUT_MS = 10_000;

/** The app's name in a pop-out's window title when the host has not said its name yet. */
export const POPOUT_APP_NAME = 'Paddock';

/**
 * Keeps `document.title` at `title()` (re-applied when anything else changes it). Returns the cleanup.
 */
export function pinDocumentTitle(title: () => string, injector: Injector): () => void {
  let applying = false;
  const apply = () => {
    const want = title();
    if (document.title === want) return;
    applying = true;
    document.title = want;
    applying = false;
  };
  // Setting the title is a side effect on the DOM; it reads only `title()`.
  const ref = effect(() => apply(), { injector, manualCleanup: true });
  const observer = new MutationObserver(() => {
    if (!applying) apply();
  });
  observer.observe(document.head ?? document.documentElement, { subtree: true, childList: true, characterData: true });
  return () => {
    ref.destroy();
    observer.disconnect();
  };
}

/**
 * The minimal app a pop-out window boots into (rule 34: its own bootstrap, its own small provider set): a title bar (the
 * panel's name, the host's name, Dock back, Keep on top) and the one view, loaded with only the providers that view declares
 * (`PdPartOutlet`, with its error and retry state). It shares the host's interaction state through `UiSync` (the registry's
 * `SharedStateDef`s), queries the engine itself (Rust resolves a pop-out's commands to its host), and closes itself when the
 * host says it is going away.
 */
@Component({
  selector: 'pd-popout-shell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdPartOutlet, PdPopoutTitleBar],
  template: `
    <pd-popout-title-bar [title]="viewTitle()" [path]="route.panel.key ?? ''" [host]="state.hostTitle()" [onTop]="state.keepOnTop()" (dockBack)="dockBack()" (toggleOnTop)="toggleOnTop()" />
    <main class="pd-view" [attr.aria-label]="viewTitle()" [attr.aria-busy]="!ready()" data-testid="popout-view">
      @if (!view()) {
        <p class="pd-note" role="alert" data-testid="popout-missing">{{ t.missing }}</p>
      } @else if (ready()) {
        <pd-part-outlet [part]="view()!" [label]="viewTitle()" />
      } @else {
        <p class="pd-note" data-testid="popout-connecting">{{ t.connecting }}</p>
      }
    </main>
    @if (problem(); as p) {
      <div class="pd-toast" role="alert" data-testid="popout-problem">
        <span>{{ p }}</span>
        <button type="button" (click)="problem.set(null)">{{ t.dismiss }}</button>
      </div>
    }
  `,
  styles: `
    :host { --pd-pane-inset: var(--pd-space-3); display: flex; flex-direction: column; height: 100vh; background: var(--pd-bg); color: var(--pd-text); }
    .pd-view { flex: 1; min-height: 0; min-width: 0; display: flex; flex-direction: column; overflow: auto; }
    .pd-note { margin: auto; color: var(--pd-text-muted); font-size: var(--pd-text-body); }
    .pd-toast { position: fixed; inset-inline-end: var(--pd-space-3); bottom: var(--pd-space-3); max-width: min(420px, calc(100vw - 2 * var(--pd-space-3))); z-index: 100; display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-2) var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-3); border: 1px solid var(--pd-danger); }
    .pd-toast button { min-height: 28px; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: transparent; color: inherit; font: inherit; cursor: pointer; }
  `,
})
export class PdPopoutShell {
  protected readonly state = inject(PopoutState);
  protected readonly route = inject(POPOUT_ROUTE)!;
  private readonly registry = inject(WorkbenchRegistry);
  private readonly io = inject(PANELS_IO);
  private readonly chrome = inject(WINDOW_CHROME);
  private readonly sync = inject(UiSync);
  private readonly timers = inject(SYNC_TIMERS);
  private readonly injector = inject(Injector);
  protected readonly t = {
    connecting: $localize`:@@pd.wb.panels.connecting:Connecting to the main window…`,
    missing: $localize`:@@pd.wb.panels.missing:This panel is not available in this version. Dock it back or close the window.`,
    dismiss: $localize`:@@pd.wb.panels.dismiss:Dismiss`,
  };
  protected readonly problem = signal<string | null>(null);
  protected readonly ready = signal(false);
  /** The view the panel shows; undefined when this version no longer has it. */
  protected readonly view = computed<ViewDef | undefined>(() => this.registry.findView(this.route.panel.panel));
  /** The panel's title, the same as its tab in the main window ("main.rs" for an instance, the view's title otherwise), never the view type. */
  protected readonly viewTitle = computed(() => {
    const view = this.view();
    return view ? panelTitle(view, this.route.panel.panel) : this.route.panel.view;
  });

  constructor() {
    const destroy = inject(DestroyRef);
    destroy.onDestroy(pinDocumentTitle(() => windowTitle(this.viewTitle(), this.state.hostTitle() || POPOUT_APP_NAME), this.injector));
    // The view appears once this window caught up with the host (or after a timeout, unsynced), so what the user does in
    // it is never overwritten by the host's snapshot.
    const mount = () => this.ready.set(true);
    const timeout = this.timers.setTimeout(mount, SYNC_START_TIMEOUT_MS);
    destroy.onDestroy(() => this.timers.clearTimeout(timeout));
    this.sync.onSynced(mount);
    this.sync.onHostGone(() => void this.chrome.close());
    const values = [
      ...syncedValues(this.registry, 'popout'),
      hostTitleValue(
        () => this.state.hostTitle(),
        (title) => this.state.hostTitle.set(title),
      ),
    ];
    // Starting the sync talks to the shell (outside the signal graph); nothing here is read reactively.
    untracked(() => this.sync.start(this.io, 'popout', this.route.label, values));
  }

  protected async dockBack(): Promise<void> {
    try {
      await this.io.dockBack();
    } catch (e) {
      this.problem.set($localize`:@@pd.wb.panels.dock-back-failed:Could not dock the panel back: ${reason(e)}:reason:`);
    }
  }

  protected async toggleOnTop(): Promise<void> {
    const on = !this.state.keepOnTop();
    try {
      await this.io.keepOnTop(on);
      this.state.keepOnTop.set(on);
    } catch (e) {
      this.problem.set($localize`:@@pd.wb.panels.on-top-failed:Could not keep the window on top: ${reason(e)}:reason:`);
    }
  }
}

const reason = (e: unknown): string => (e instanceof Error ? e.message : typeof e === 'object' && e !== null && 'message' in e ? String((e as { message: unknown }).message) : String(e));

export { HOST_TITLE_KEY };
