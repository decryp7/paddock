import { ChangeDetectionStrategy, Component, ElementRef, Injectable, effect, inject, signal, viewChild } from '@angular/core';

/** The open Copy menu: where, and what it copies. */
export interface CopyMenuState {
  x: number;
  y: number;
  text: string;
}

/** Spotter's own context menu for selected text (ADR 0008): one item, Copy. Opened by the native-feel guard. */
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
  selector: 'sp-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'menu.close()', '(document:mousedown)': 'outside($event)' },
  template: `
    @if (menu.state(); as s) {
      <div class="menu" role="menu" [style.left.px]="s.x" [style.top.px]="s.y">
        <button #item type="button" role="menuitem" (click)="menu.copy()" (blur)="menu.close()">Copy</button>
      </div>
    }
  `,
  styles: `
    .menu {
      position: fixed;
      z-index: var(--z-toast);
      min-width: 112px;
      padding: var(--space-1);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-md);
      background: var(--surface);
      box-shadow: var(--shadow-lg);
    }
    button {
      display: block;
      width: 100%;
      padding: var(--space-1) var(--space-3);
      border: 0;
      border-radius: var(--radius-sm);
      background: transparent;
      color: var(--text);
      font: inherit;
      text-align: left;
    }
    button:hover,
    button:focus-visible {
      background: var(--accent-soft);
    }
  `,
})
export class CopyMenuHost {
  protected readonly menu = inject(CopyMenu);
  private readonly item = viewChild<ElementRef<HTMLButtonElement>>('item');

  constructor() {
    effect(() => this.item()?.nativeElement.focus());
  }

  protected outside(e: MouseEvent): void {
    if (this.menu.state() && !(e.target instanceof Element && e.target.closest('sp-copy-menu'))) this.menu.close();
  }
}
