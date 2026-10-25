import { ChangeDetectionStrategy, Component, DestroyRef, inject, input, output, signal } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { WINDOW_CHROME, type ResizeDirection } from './chrome';
import { POPOUT_ROUTE } from './popout-route';
import { UiSync } from './ui-sync';

const EDGES: readonly { dir: ResizeDirection; cls: string }[] = [
  { dir: 'North', cls: 'n' },
  { dir: 'South', cls: 's' },
  { dir: 'West', cls: 'w' },
  { dir: 'East', cls: 'e' },
  { dir: 'NorthWest', cls: 'nw' },
  { dir: 'NorthEast', cls: 'ne' },
  { dir: 'SouthWest', cls: 'sw' },
  { dir: 'SouthEast', cls: 'se' },
];

/**
 * A pop-out window's own compact title bar (IDE tool-window style): drag it to move the window (and drop it on a guide of
 * the host window to dock it), double-click to maximise or restore, Dock back, Keep on top of the host window. On macOS
 * the native traffic lights sit at its left (overlay title bar); on Windows and Linux the window is frameless and this bar
 * draws minimise / maximise / close and the resize edges. It dims while the window is inactive.
 */
@Component({
  selector: 'pd-popout-title-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  host: { '[class.pd-inactive]': '!active()', '[class.pd-mac]': "chrome.kind === 'overlay'", '[class.pd-frameless]': "chrome.kind === 'frameless'" },
  template: `
    <header class="pd-bar" data-testid="popout-bar" (pointerdown)="dragStart($event)" (dblclick)="dblclick($event)">
      <div class="pd-titles">
        <h1 class="pd-title" data-testid="popout-title" [attr.title]="path() || null">{{ title() }}</h1>
        <span class="pd-host" [attr.title]="host() || null" data-testid="popout-host">{{ host() }}</span>
      </div>
      <button type="button" class="pd-btn" data-testid="popout-dock-back" [attr.aria-label]="t.dockBack" [attr.title]="t.dockBack" (click)="dockBack.emit()">
        <pd-glyph name="dock-in" [size]="14" />
        <span class="pd-wide" aria-hidden="true">{{ t.dockBackShort }}</span>
      </button>
      @if (chrome.supportsKeepOnTop) {
        <button type="button" class="pd-btn pd-icon" data-testid="popout-on-top" [class.pd-on]="onTop()" [attr.aria-pressed]="onTop()" [attr.aria-label]="t.keepOnTop" [attr.title]="onTop() ? t.keepOnTopOn : t.keepOnTop" (click)="toggleOnTop.emit()">
          <pd-glyph name="layers" [size]="14" />
        </button>
      }
      @if (chrome.kind === 'frameless') {
        <div class="pd-controls" role="group" [attr.aria-label]="t.window">
          <button type="button" class="pd-btn pd-ctl" data-testid="popout-minimise" [attr.aria-label]="t.minimise" [attr.title]="t.minimise" (click)="chrome.minimize()">
            <pd-glyph name="minus" [size]="14" />
          </button>
          <button type="button" class="pd-btn pd-ctl" data-testid="popout-maximise" [attr.aria-label]="maximized() ? t.restore : t.maximise" [attr.title]="maximized() ? t.restore : t.maximise" (click)="toggleMaximize()">
            <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
              @if (maximized()) {
                <path d="M4 4V2h8v8h-2M2 4h8v8H2z" fill="none" stroke="currentColor" />
              } @else {
                <path d="M2 2h10v10H2z" fill="none" stroke="currentColor" />
              }
            </svg>
          </button>
          <button type="button" class="pd-btn pd-ctl pd-close" data-testid="popout-close" [attr.aria-label]="t.close" [attr.title]="t.closeTitle" (click)="chrome.close()">
            <pd-glyph name="close" [size]="14" />
          </button>
        </div>
      }
    </header>
    @if (chrome.kind === 'frameless' && !maximized()) {
      @for (e of edges; track e.dir) {
        <div class="pd-edge pd-{{ e.cls }}" aria-hidden="true" (pointerdown)="resize($event, e.dir)"></div>
      }
    }
  `,
  styles: `
    :host { display: block; flex: none; container-type: inline-size; }
    .pd-bar { display: flex; align-items: center; gap: var(--pd-space-0); height: 32px; padding-inline: var(--pd-space-3); user-select: none; background: var(--pd-surface-2); color: var(--pd-text); border-bottom: 1px solid var(--pd-border-strong); box-shadow: inset 0 2px 0 var(--pd-accent); }
    :host(.pd-frameless) .pd-bar { padding-inline-end: 0; }
    :host(.pd-mac) .pd-bar { padding-inline-start: 78px; }
    :host(.pd-inactive) .pd-bar { box-shadow: none; color: var(--pd-text-muted); }
    .pd-titles { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: var(--pd-space-2); cursor: default; }
    .pd-title { margin: 0; font-size: var(--pd-text-label); font-weight: var(--pd-weight-semibold); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
    .pd-host { font-size: var(--pd-text-caption); color: var(--pd-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
    .pd-btn { height: 28px; min-width: 28px; padding: 0 var(--pd-space-1); gap: var(--pd-space-1); border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }
    .pd-btn.pd-on { color: var(--pd-accent); background: var(--pd-accent-tint); }
    .pd-btn:hover { color: var(--pd-text); background: var(--pd-surface-3); }
    .pd-btn.pd-on:hover { background: var(--pd-accent-tint); }
    .pd-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    .pd-wide { display: none; }
    @container (min-width: 480px) { .pd-wide { display: inline; } }
    .pd-controls { display: flex; height: 100%; margin-inline-start: var(--pd-space-1); }
    .pd-ctl { width: 40px; height: 100%; border-radius: 0; color: inherit; }
    .pd-close:hover { background: var(--pd-danger); color: var(--pd-on-danger); }
    .pd-edge { position: fixed; z-index: 1000; }
    .pd-n { top: 0; left: 6px; right: 6px; height: 4px; cursor: ns-resize; }
    .pd-s { bottom: 0; left: 6px; right: 6px; height: 4px; cursor: ns-resize; }
    .pd-w { left: 0; top: 6px; bottom: 6px; width: 4px; cursor: ew-resize; }
    .pd-e { right: 0; top: 6px; bottom: 6px; width: 4px; cursor: ew-resize; }
    .pd-nw { top: 0; left: 0; width: 6px; height: 6px; cursor: nwse-resize; }
    .pd-ne { top: 0; right: 0; width: 6px; height: 6px; cursor: nesw-resize; }
    .pd-sw { bottom: 0; left: 0; width: 6px; height: 6px; cursor: nesw-resize; }
    .pd-se { bottom: 0; right: 0; width: 6px; height: 6px; cursor: nwse-resize; }
  `,
})
export class PdPopoutTitleBar {
  protected readonly chrome = inject(WINDOW_CHROME);
  private readonly sync = inject(UiSync);
  private readonly route = inject(POPOUT_ROUTE);
  /** The panel's name. */
  readonly title = input.required<string>();
  /** The panel's full path or key (the tooltip of the title); empty for a view without instances. */
  readonly path = input('');
  /** The host window's name. */
  readonly host = input('');
  readonly onTop = input(false);
  readonly dockBack = output<void>();
  readonly toggleOnTop = output<void>();
  protected readonly edges = EDGES;
  protected readonly t = {
    dockBack: $localize`:@@pd.wb.panels.dock-back:Dock back into the main window`,
    dockBackShort: $localize`:@@pd.wb.panels.dock-back-short:Dock back`,
    keepOnTop: $localize`:@@pd.wb.panels.keep-on-top:Keep on top of the main window`,
    keepOnTopOn: $localize`:@@pd.wb.panels.keep-on-top-on:Keep on top of the main window (on): click to stop`,
    window: $localize`:@@pd.wb.panels.window:Window`,
    minimise: $localize`:@@pd.wb.panels.minimise:Minimise`,
    maximise: $localize`:@@pd.wb.panels.maximise:Maximise`,
    restore: $localize`:@@pd.wb.panels.restore:Restore`,
    close: $localize`:@@pd.wb.panels.close:Close (dock back)`,
    closeTitle: $localize`:@@pd.wb.panels.close-title:Close (the panel docks back)`,
  };
  protected readonly maximized = signal(false);
  protected readonly active = signal(typeof document === 'undefined' || document.hasFocus());

  constructor() {
    const destroy = inject(DestroyRef);
    const focus = () => this.active.set(true);
    const blur = () => this.active.set(false);
    window.addEventListener('focus', focus);
    window.addEventListener('blur', blur);
    const refresh = () => void this.chrome.isMaximized().then((m) => this.maximized.set(m), () => undefined);
    refresh();
    let off: (() => void) | null = null;
    let gone = false;
    void this.chrome.onResized(refresh).then(
      (u) => (gone ? u() : (off = u)),
      () => undefined,
    );
    destroy.onDestroy(() => {
      gone = true;
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', blur);
      off?.();
    });
  }

  protected async toggleMaximize(): Promise<void> {
    await this.chrome.toggleMaximize();
    this.maximized.set(await this.chrome.isMaximized());
  }

  /** Double-clicking the bar (not a button) maximises or restores, as on a native title bar. */
  protected dblclick(e: MouseEvent): void {
    if ((e.target as Element).closest('button') || this.chrome.kind === 'browser') return;
    void this.toggleMaximize();
  }

  protected resize(e: PointerEvent, direction: ResizeDirection): void {
    if (e.button !== 0) return;
    e.preventDefault();
    void this.chrome.startResize(direction);
  }

  /**
   * Pressing the bar (not a button) moves the window: the OS moves it in the app (and the host shows its dock guides while it
   * is over it; Rust reports the cursor). A browser window cannot be moved by the page, so there the pointer's screen
   * position is sent to the host instead (same guides, same docking).
   */
  protected async dragStart(e: PointerEvent): Promise<void> {
    if (e.button !== 0 || e.detail > 1 || (e.target as Element).closest('button')) return;
    const label = this.route?.label ?? '';
    if (this.chrome.kind !== 'browser') {
      e.preventDefault();
      if (await this.chrome.startDragging().catch(() => false)) this.sync.runInHost('dragEnd', label);
      return;
    }
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic events have no capturable pointer */
    }
    const move = (m: PointerEvent) => this.sync.runInHost('dragOver', JSON.stringify({ label, x: m.screenX, y: m.screenY }));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      this.sync.runInHost('dragEnd', label);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }
}
