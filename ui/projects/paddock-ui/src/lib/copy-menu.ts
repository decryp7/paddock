import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import { afterNextRender, ChangeDetectionStrategy, Component, effect, inject, Injectable, Injector, signal, untracked, viewChild } from '@angular/core';
import { copyText } from './clipboard';

/** The open Copy menu: where, and what it copies. */
export interface PdCopyMenuState {
  x: number;
  y: number;
  text: string;
  /** The right click that opened it, if any: the CDK menu then ignores that click's own release (see `open`). */
  event?: MouseEvent;
}

/**
 * The app's own context menu for selected text: one item, Copy (plan X35). The native-feel guard opens it on a right-click
 * inside a selection, because the webview's own menu is switched off; `<pd-copy-menu />` draws it (the pages frame has one).
 */
@Injectable({ providedIn: 'root' })
export class PdCopyMenu {
  private readonly stateSignal = signal<PdCopyMenuState | null>(null);
  /** The open menu, or null. */
  readonly state = this.stateSignal.asReadonly();

  /**
   * Opens the menu at the pointer for `text`. Pass the `contextmenu` event that asked for it: where that event comes on the
   * button press (macOS and Linux, so WKWebView and WebKitGTK), the release follows once the menu is drawn, and its
   * `auxclick` outside the menu would close it again at once.
   */
  open(x: number, y: number, text: string, event?: MouseEvent): void {
    this.stateSignal.set(event ? { x, y, text, event } : { x, y, text });
  }

  /** Closes the menu. */
  close(): void {
    this.stateSignal.set(null);
  }

  /** Copies the text and closes the menu; a clipboard that is not available only leaves the menu closed (Ctrl/Cmd+C still works). */
  async copy(): Promise<void> {
    const s = this.stateSignal();
    this.close();
    if (s) await copyText(s.text);
  }
}

/**
 * Draws the Copy menu at the pointer as a CDK context menu: it focuses Copy, and Escape, a click elsewhere or losing focus
 * closes it (the CDK menu's own behaviour).
 */
@Component({
  selector: 'pd-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkContextMenuTrigger, CdkMenu, CdkMenuItem],
  template: `
    <span class="pd-anchor" [cdkContextMenuTriggerFor]="panel" (cdkContextMenuClosed)="menu.close()"></span>
    <ng-template #panel>
      <div cdkMenu class="pd-menu" [attr.aria-label]="label">
        <button type="button" cdkMenuItem class="pd-item" (cdkMenuItemTriggered)="menu.copy()">{{ label }}</button>
      </div>
    </ng-template>
  `,
  styles: `
    .pd-anchor { display: none; }
    .pd-menu { min-width: 112px; padding: var(--pd-space-1); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); box-shadow: var(--pd-shadow-2); }
    .pd-menu:focus { outline: none; }
    .pd-item { display: block; width: 100%; padding: var(--pd-space-1) var(--pd-space-3); border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text); font: inherit; text-align: start; cursor: pointer; }
    .pd-item:hover { background: var(--pd-hover); }
    .pd-item:focus-visible { background: var(--pd-hover); outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
  `,
})
export class PdCopyMenuHost {
  protected readonly menu = inject(PdCopyMenu);
  protected readonly label = $localize`:@@pd.copyMenu.copy:Copy`;
  private readonly trigger = viewChild.required(CdkContextMenuTrigger);
  /** The menu panel, once it is open (its template is declared here, so the query finds it in the overlay). */
  private readonly panel = viewChild(CdkMenu);
  private readonly injector = inject(Injector);

  constructor() {
    // The service's state is the one truth: the CDK menu is opened and closed to follow it (a DOM overlay is outside the signal graph).
    effect(() => {
      const s = this.menu.state();
      const trigger = this.trigger();
      untracked(() => {
        if (!s) {
          trigger.close();
          return;
        }
        // Opened with the right click itself, the CDK skips that click's own auxclick (the CDK's `_openOnContextMenu`, its
        // public host listener); a programmatic open has none to skip.
        if (s.event) trigger._openOnContextMenu(s.event);
        else trigger.open({ x: s.x, y: s.y });
        afterNextRender(() => this.panel()?.focusFirstItem('program'), { injector: this.injector });
      });
    });
  }
}
