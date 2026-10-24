import { ChangeDetectionStrategy, Component, ElementRef, effect, inject, Injectable, signal, viewChild } from '@angular/core';
import { copyText } from './clipboard';

/** The open Copy menu: where, and what it copies. */
export interface PdCopyMenuState {
  x: number;
  y: number;
  text: string;
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

  /** Opens the menu at the pointer for `text`. */
  open(x: number, y: number, text: string): void {
    this.stateSignal.set({ x, y, text });
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

/** Draws the Copy menu at the pointer; Escape, a click elsewhere or losing focus closes it. */
@Component({
  selector: 'pd-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'menu.close()', '(document:mousedown)': 'outside($event)' },
  template: `
    @if (menu.state(); as s) {
      <div class="pd-menu" role="menu" [style.left.px]="s.x" [style.top.px]="s.y">
        <button #item type="button" role="menuitem" class="pd-item" (click)="menu.copy()" (blur)="menu.close()">{{ label }}</button>
      </div>
    }
  `,
  styles: `
    .pd-menu { position: fixed; z-index: var(--pd-z-toast); min-width: 112px; padding: var(--pd-space-1); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); box-shadow: var(--pd-shadow-2); }
    .pd-item { display: block; width: 100%; padding: var(--pd-space-1) var(--pd-space-3); border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text); font: inherit; text-align: start; cursor: pointer; }
    .pd-item:hover, .pd-item:focus-visible { background: var(--pd-hover); outline: none; }
  `,
})
export class PdCopyMenuHost {
  protected readonly menu = inject(PdCopyMenu);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly label = $localize`:@@pd.copyMenu.copy:Copy`;
  private readonly item = viewChild<ElementRef<HTMLButtonElement>>('item');

  constructor() {
    // Focus is the one thing outside the signal graph this does: the item takes it when the menu opens.
    effect(() => this.item()?.nativeElement.focus());
  }

  protected outside(e: MouseEvent): void {
    if (this.menu.state() && !(e.target instanceof Node && this.host.contains(e.target))) this.menu.close();
  }
}
