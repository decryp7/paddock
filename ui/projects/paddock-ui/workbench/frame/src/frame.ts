import { DOCUMENT } from '@angular/common';
import { Title } from '@angular/platform-browser';
import { afterNextRender, ChangeDetectionStrategy, Component, DestroyRef, ElementRef, inject } from '@angular/core';
import { PdCopyMenuHost } from '@decryp7/paddock-ui';
import { cycleRegions, PdStatusBar, PdTitleBar, regionKey } from '@decryp7/paddock-ui/shell';
import { DialogHost } from '@decryp7/paddock-ui/shell/dialogs';
import { MENU_MODE, PdMenuBar } from '@decryp7/paddock-ui/shell/menu';
import { PdNotificationHost } from '@decryp7/paddock-ui/shell/notify';
import { PdRecoveryNotice, PdSafeModeBadge, RecoveryStore } from '@decryp7/paddock-ui/shell/startup';
import { EXPLORER_PLAN, WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';
import { PdExplorer } from '@decryp7/paddock-ui/workbench/explorer';
import { PdCommandPalette } from '@decryp7/paddock-ui/workbench/palette';
import { PdActivityBar } from './activity-bar';
import { EXPLORER_MAX_PX, EXPLORER_MIN_PX, WorkbenchFrameStore } from './frame.store';
import { WORKBENCH_CONFIG } from './provide-workbench';

/**
 * The window of an IDE-style app (ADR 0013 §1; plan W4; Debrief's `app.ts` frame): the title bar with the menu bar, the
 * activity bar, the Explorer side bar (contributed sections; opened, closed and sized by the user, closed and reopened by
 * the layout plan when the window is narrow), the stage the app fills with the dock (`<pd-dock slot="stage" />`), the
 * status bar with the contributed items, the command palette, notifications and dialogs. F6 cycles the regions. The
 * pages frame and this frame share the title bar, status bar and regions (plan §5.5).
 *
 * Slots: `[slot=stage]` (the dock), `[slot=welcome]` (over the stage, e.g. a first-run page), `[slot=title-end]`.
 */
@Component({
  selector: 'pd-workbench',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    PdTitleBar, PdMenuBar, PdStatusBar, PdNotificationHost, DialogHost, PdRecoveryNotice, PdSafeModeBadge, PdCopyMenuHost, PdActivityBar, PdExplorer, PdCommandPalette,
  ],
  providers: [RecoveryStore],
  host: {
    '[class.pd-native-menu]': 'menuMode === "native"',
    '[attr.data-explorer]': 'frame.explorerShown() ?? "closed"',
    '(document:keydown)': 'onKey($event)',
  },
  template: `
    <a class="pd-skip" href="#pd-stage" (click)="skip($event)">{{ t.skip }}</a>
    <header class="pd-title" data-pd-region="title" tabindex="-1">
      <pd-title-bar [appName]="config.appName">
        <pd-menu-bar />
        <ng-container ngProjectAs="[slot=end]"><pd-safe-mode-badge /><ng-content select="[slot=title-end]" /></ng-container>
      </pd-title-bar>
      <pd-recovery-notice />
    </header>
    <div class="pd-body">
      <pd-activity-bar role="navigation" [attr.aria-label]="t.activity" data-pd-region="activity" tabindex="-1" />
      @if (frame.explorerShown()) {
        <aside class="pd-side" role="complementary" [attr.aria-label]="t.sidebar" data-pd-region="sidebar" tabindex="-1" data-testid="workbench-sidebar" [style.width.px]="frame.explorerWidth()">
          <pd-explorer />
        </aside>
        <div
          class="pd-sash"
          role="separator"
          aria-orientation="vertical"
          tabindex="0"
          [attr.aria-label]="t.resize"
          [attr.aria-valuenow]="frame.explorerWidth()"
          [attr.aria-valuemin]="minWidth"
          [attr.aria-valuemax]="maxWidth"
          data-testid="sidebar-sash"
          (pointerdown)="startResize($event)"
          (keydown)="resizeKey($event)"
        ></div>
      }
      <main id="pd-stage" class="pd-stage" data-pd-region="main" tabindex="-1">
        <ng-content select="[slot=stage]" />
        <ng-content select="[slot=welcome]" />
      </main>
    </div>
    <footer class="pd-status" [attr.aria-label]="t.statusBar" data-pd-region="status" tabindex="-1">
      <pd-status-bar />
    </footer>
    <pd-command-palette />
    <pd-notification-host />
    <pd-dialog-host />
    <pd-copy-menu />
  `,
  styles: `
    :host { --pd-titlebar-height: 40px; container: pd-workbench / inline-size; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; height: 100vh; height: 100dvh; min-width: 0; overflow: hidden; background: var(--pd-bg); color: var(--pd-text); }
    :host(.pd-native-menu) { --pd-titlebar-inset-start: 78px; }
    .pd-title { min-width: 0; background: var(--pd-surface-1); border-bottom: 1px solid var(--pd-hairline); outline: none; }
    .pd-body { display: flex; min-height: 0; min-width: 0; }
    .pd-side { flex: none; display: flex; flex-direction: column; min-height: 0; min-width: 0; max-width: 50%; background: var(--pd-surface-1); outline: none; }
    .pd-side > pd-explorer { flex: 1; }
    .pd-sash { flex: none; width: 4px; margin-inline-start: -2px; margin-inline-end: -2px; z-index: 1; cursor: col-resize; background: transparent; border-inline-start: 1px solid var(--pd-hairline); touch-action: none; }
    .pd-sash:hover, .pd-sash:focus-visible { background: var(--pd-accent); outline: none; }
    /* Views lay out by container queries on the stage (container pd-stage), not the window (rule 18). */
    .pd-stage { container: pd-stage / inline-size; position: relative; flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; isolation: isolate; outline: none; background: var(--pd-bg); }
    .pd-status { min-width: 0; overflow-x: clip; background: var(--pd-surface-1); border-top: 1px solid var(--pd-hairline); outline: none; }
    .pd-title:focus-visible, .pd-side:focus-visible, .pd-stage:focus-visible, .pd-status:focus-visible, pd-activity-bar:focus-visible { box-shadow: inset 0 0 0 2px var(--pd-focus-ring); outline: none; }
    .pd-skip { position: fixed; inset-inline-start: var(--pd-space-3); top: -48px; z-index: var(--pd-z-toast); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-accent-fill); color: var(--pd-on-accent); font-weight: var(--pd-weight-medium); }
    .pd-skip:focus { top: var(--pd-space-2); text-decoration: none; }
    /* Below 480 px the activity bar narrows (rule 18: nothing scrolls sideways at 320 px). */
    @container pd-workbench (max-width: 480px) { pd-activity-bar { width: 40px; } }
  `,
})
export class PdWorkbench {
  protected readonly t = {
    skip: $localize`:@@pd.wb.frame.skip:Skip to the work area`,
    activity: $localize`:@@pd.wb.frame.activity:Activity bar`,
    sidebar: $localize`:@@pd.wb.frame.sidebar:Side bar`,
    resize: $localize`:@@pd.wb.frame.resize:Resize the side bar`,
    statusBar: $localize`:@@pd.wb.frame.status:Status bar`,
  };
  protected readonly frame = inject(WorkbenchFrameStore);
  protected readonly config = inject(WORKBENCH_CONFIG);
  protected readonly menuMode = inject(MENU_MODE);
  protected readonly minWidth = EXPLORER_MIN_PX;
  protected readonly maxWidth = EXPLORER_MAX_PX;
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly doc = inject(DOCUMENT);

  constructor() {
    const registry = inject(WorkbenchRegistry);
    inject(Title).setTitle(this.config.appName);
    this.frame.init(registry.availableExplorerProviders()[0]?.id ?? registry.explorerProviders[0]?.id ?? null);
    const measure = () => this.frame.evaluate(this.el.clientWidth);
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(measure) : null;
    const plan = inject(EXPLORER_PLAN);
    const unplan = plan?.onWindowPlan(measure);
    afterNextRender(() => {
      measure();
      observer?.observe(this.el);
    });
    inject(DestroyRef).onDestroy(() => {
      observer?.disconnect();
      unplan?.();
    });
  }

  protected onKey(e: KeyboardEvent): void {
    if (e.defaultPrevented) return;
    const direction = regionKey(e);
    if (direction === null) return;
    e.preventDefault();
    cycleRegions(this.el, direction, this.doc.activeElement);
  }

  protected skip(e: Event): void {
    e.preventDefault();
    this.el.querySelector<HTMLElement>('#pd-stage')?.focus();
  }

  /** Drags the sash: the width follows the pointer until it is released. */
  protected startResize(e: PointerEvent): void {
    const sash = e.currentTarget as HTMLElement;
    const startX = e.clientX;
    const startWidth = this.frame.explorerWidth();
    const rtl = this.doc.dir === 'rtl';
    sash.setPointerCapture?.(e.pointerId);
    const move = (m: PointerEvent) => this.frame.setWidth(startWidth + (rtl ? startX - m.clientX : m.clientX - startX));
    const up = () => {
      sash.removeEventListener('pointermove', move);
      sash.removeEventListener('pointerup', up);
      sash.removeEventListener('pointercancel', up);
      this.frame.evaluate(this.el.clientWidth);
    };
    sash.addEventListener('pointermove', move);
    sash.addEventListener('pointerup', up);
    sash.addEventListener('pointercancel', up);
    e.preventDefault();
  }

  /** Arrow keys resize by 16 px (Shift: 64), Home and End go to the limits. */
  protected resizeKey(e: KeyboardEvent): void {
    const step = e.shiftKey ? 64 : 16;
    const w = this.frame.explorerWidth();
    const next = e.key === 'ArrowLeft' ? w - step : e.key === 'ArrowRight' ? w + step : e.key === 'Home' ? EXPLORER_MIN_PX : e.key === 'End' ? EXPLORER_MAX_PX : null;
    if (next === null) return;
    e.preventDefault();
    this.frame.setWidth(next);
    this.frame.evaluate(this.el.clientWidth);
  }
}
