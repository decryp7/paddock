import { DOCUMENT } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, Injectable, Injector, afterNextRender, effect, inject, signal, untracked, viewChild } from '@angular/core';

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

/**
 * Renders the Copy menu at the pointer; Escape, a click elsewhere or losing focus closes it. Closed, it gives back the text
 * selection it opened on, as a native menu keeps it: WebKit (WKWebView) drops the selection when focus moves into the menu.
 * A selection the user made meanwhile stays.
 */
@Component({
  selector: 'sp-copy-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'menu.close()', '(document:mousedown)': 'outside($event)' },
  template: `
    @if (menu.state(); as s) {
      <div class="menu" role="menu" [style.left.px]="s.x" [style.top.px]="s.y">
        <!-- WebKit does not focus a pressed button but blurs the focused one: the press keeps the focus, or the blur would close the menu first. -->
        <button #item type="button" role="menuitem" (mousedown)="$event.preventDefault()" (click)="menu.copy()" (blur)="menu.close()">Copy</button>
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
  private readonly document = inject(DOCUMENT);
  private readonly injector = inject(Injector);
  /** The selection when the menu opened (copies of its ranges), given back when it closes; null while closed. */
  private saved: Range[] | null = null;

  constructor() {
    // Before the focus moves into the menu (the item renders after this runs): save the selection; on close, give it back.
    effect(() => {
      const open = this.menu.state() !== null;
      untracked(() => {
        if (open) {
          this.saved ??= this.saveSelection();
          return;
        }
        const saved = this.saved;
        this.saved = null;
        if (saved) afterNextRender(() => this.restoreSelection(saved), { injector: this.injector });
      });
    });
    effect(() => this.item()?.nativeElement.focus());
  }

  private saveSelection(): Range[] {
    const sel = this.document.getSelection();
    if (!sel) return [];
    return Array.from({ length: sel.rangeCount }, (_, i) => sel.getRangeAt(i).cloneRange());
  }

  /** Puts the saved ranges back unless the user selected something else meanwhile (a selection that is not just a caret). */
  private restoreSelection(saved: readonly Range[]): void {
    const sel = this.document.getSelection();
    if (!sel || saved.length === 0 || (sel.rangeCount > 0 && !sel.isCollapsed)) return;
    const live = saved.filter((r) => r.startContainer.isConnected && r.endContainer.isConnected);
    if (live.length === 0) return;
    sel.removeAllRanges();
    for (const r of live) sel.addRange(r);
  }

  protected outside(e: MouseEvent): void {
    if (this.menu.state() && !(e.target instanceof Element && e.target.closest('sp-copy-menu'))) this.menu.close();
  }
}
