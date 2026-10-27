import { ChangeDetectionStrategy, Component, ElementRef, Injectable, effect, inject, signal, viewChild } from '@angular/core';

/** The open Copy menu: where, and what it copies. */
export interface CopyMenuState {
  x: number;
  y: number;
  text: string;
}

/** Dyno's own context menu for selected text (native feel): one item, Copy. Opened by the native-feel guard. */
@Injectable({ providedIn: 'root' })
export class CopyMenu {
  readonly state = signal<CopyMenuState | null>(null);

  open(x: number, y: number, text: string): void {
    this.state.set({ x, y, text });
  }

  close(): void {
    this.state.set(null);
  }

  /** Copies the text and closes the menu; a clipboard that is not available only leaves the menu closed. */
  async copy(): Promise<void> {
    const s = this.state();
    this.close();
    if (!s) return;
    try {
      await navigator.clipboard.writeText(s.text);
    } catch {
      // The selection stays; Ctrl/Cmd+C still copies it.
    }
  }
}

/** Renders the Copy menu at the pointer; Escape, a click elsewhere or losing focus closes it. */
@Component({
  selector: 'dy-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'menu.close()', '(document:mousedown)': 'outside($event)' },
  template: `
    @if (menu.state(); as s) {
      <div class="menu copy" role="menu" [style.left.px]="s.x" [style.top.px]="s.y">
        <button #item type="button" role="menuitem" class="menu-item" (click)="menu.copy()" (blur)="menu.close()">Copy</button>
      </div>
    }
  `,
  styles: `
    .copy { position: fixed; z-index: 1200; min-width: 112px; }
  `,
})
export class CopyMenuHost {
  protected readonly menu = inject(CopyMenu);
  private readonly item = viewChild<ElementRef<HTMLButtonElement>>('item');

  constructor() {
    effect(() => this.item()?.nativeElement.focus());
  }

  protected outside(e: MouseEvent): void {
    if (this.menu.state() && !(e.target instanceof Element && e.target.closest('dy-copy-menu'))) this.menu.close();
  }
}
