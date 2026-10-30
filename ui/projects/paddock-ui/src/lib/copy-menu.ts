import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import { DOCUMENT } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, DestroyRef, effect, inject, Injectable, Injector, signal, untracked, viewChild } from '@angular/core';
import { copyText } from './clipboard';
import { preserveSelection, type PreservedSelection } from './selection';

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
  private readonly document = inject(DOCUMENT);
  private readonly injector = inject(Injector);
  /** The selection the menu opened on (see `close`). */
  private selection: PreservedSelection | null = null;

  /**
   * Opens the menu at the pointer for `text`. Pass the `contextmenu` event that asked for it: where that event comes on the
   * button press (macOS and Linux, so WKWebView and WebKitGTK), the release follows once the menu is drawn, and its
   * `auxclick` outside the menu would close it again at once.
   */
  open(x: number, y: number, text: string, event?: MouseEvent): void {
    this.selection ??= preserveSelection(this.document);
    this.stateSignal.set(event ? { x, y, text, event } : { x, y, text });
  }

  /**
   * Closes the menu. Closed by Escape, by Copy or by the app (`restoreSelection` true), it gives back the text selection it
   * opened on, as a native menu keeps it: WebKit drops the selection when focus moves into the menu. A press elsewhere
   * (`false`) leaves the selection to that press, and a focused text field always keeps its caret (`preserveSelection`).
   */
  close(restoreSelection = true): void {
    const selection = this.selection;
    this.selection = null;
    this.stateSignal.set(null);
    // After the render, so after the CDK has moved the focus back (which in WebKit can clear the selection again).
    if (selection && restoreSelection) afterNextRender(() => selection.restore(), { injector: this.injector });
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
 * closes it (the CDK menu's own behaviour). A press outside the menu is remembered, so that close leaves the selection alone.
 */
@Component({
  selector: 'pd-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkContextMenuTrigger, CdkMenu, CdkMenuItem],
  template: `
    <span class="pd-anchor" [cdkContextMenuTriggerFor]="panel" (cdkContextMenuClosed)="closed()"></span>
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
  /**
   * The event being dispatched is a click, auxclick or right click outside the open menu: the very events the CDK closes the
   * menu on. It describes only the close that event causes, never a later Escape (see the listeners in the constructor).
   */
  private dismissing = false;

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
        this.dismissing = false;
        // Opened with the right click itself, the CDK skips that click's own auxclick. `_openOnContextMenu` is the CDK's host
        // listener: public in its typings but not documented API, so the parts.spec test "stays open through the release of the
        // right click" guards it on every CDK update.
        if (s.event) trigger._openOnContextMenu(s.event);
        else trigger.open({ x: s.x, y: s.y });
        afterNextRender(() => this.panel()?.focusFirstItem('program'), { injector: this.injector });
      });
    });
    const doc = inject(DOCUMENT);
    const win = doc.defaultView;
    // Set before the CDK's listener on the events it closes on; cleared after them on the window, and at the start of the next
    // press or key (an app that stopped a click's propagation kept both the CDK and the window from seeing it).
    const closing = ['click', 'auxclick', 'contextmenu'] as const;
    const fresh = ['pointerdown', 'keydown'] as const;
    const before = (e: Event) => {
      this.dismissing = this.menu.state() !== null && !(e.target instanceof Element && e.target.closest('.pd-menu'));
    };
    const clear = () => (this.dismissing = false);
    for (const t of closing) {
      doc.addEventListener(t, before, true);
      win?.addEventListener(t, clear);
    }
    for (const t of fresh) doc.addEventListener(t, clear, true);
    inject(DestroyRef).onDestroy(() => {
      for (const t of closing) {
        doc.removeEventListener(t, before, true);
        win?.removeEventListener(t, clear);
      }
      for (const t of fresh) doc.removeEventListener(t, clear, true);
    });
  }

  /** The CDK closed the menu: a dismissal by a click elsewhere leaves the selection alone; Escape and Copy give it back. */
  protected closed(): void {
    const dismissed = this.dismissing;
    this.dismissing = false;
    this.menu.close(!dismissed);
  }
}
