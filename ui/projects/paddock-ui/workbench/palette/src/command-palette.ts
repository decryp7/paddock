import { DOCUMENT } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, Injector, signal, untracked, viewChild } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';
import { PdIcon } from '@decryp7/paddock-ui/shell';
import { titleParts, type TitlePart } from './match';
import { PaletteStore, type PaletteRow } from './palette.store';

let nextId = 0;

/**
 * The command palette (plan W5; Debrief's `command-palette.ts`): an in-app overlay (rule 17 keeps pickers in the window)
 * with a combobox and a list of results; the highlight is `aria-activedescendant`, focus stays in the input. Rows are
 * text bound as text, matches highlighted with `<mark>`. The input and the list keep their place while a provider
 * searches (Debrief ADR 0061): a status line under the input says "Searching" and an error there offers Retry.
 */
@Component({
  selector: 'pd-command-palette',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdIcon, PdPathText],
  host: { '(document:keydown)': 'onEarlyKey($event)' },
  template: `
    @if (store.isOpen()) {
      <div class="pd-pal-backdrop" data-testid="palette-backdrop" (mousedown)="close()"></div>
      <div class="pd-pal" role="dialog" aria-modal="true" [attr.aria-label]="t.dialog" data-testid="palette">
        <input
          #input
          class="pd-pal-input"
          role="combobox"
          [attr.aria-label]="t.input"
          aria-autocomplete="list"
          [attr.aria-expanded]="store.rows().length > 0"
          [attr.aria-controls]="store.rows().length ? listId : null"
          [attr.aria-activedescendant]="activeId()"
          [placeholder]="store.placeholder()"
          autocomplete="off"
          spellcheck="false"
          data-testid="palette-input"
          [value]="store.query()"
          (input)="store.setQuery($any($event.target).value)"
          (focus)="onFocus($event)"
          (blur)="onBlur()"
          (keydown)="onKey($event)"
        />
        <div class="pd-pal-status" role="status" data-testid="palette-status">
          @if (store.isLoading()) {
            <span class="pd-pal-searching">{{ t.searching }}</span>
          } @else if (store.requestError(); as error) {
            <span class="pd-pal-error">{{ error }}</span>
            <button type="button" class="pd-pal-retry" data-testid="palette-retry" (click)="store.retry()">{{ t.retry }}</button>
          } @else if (store.suggesting()) {
            <span data-testid="palette-did-you-mean">{{ didYouMean() }}</span>
          } @else if (!store.rows().length) {
            <!-- Said in the live status (a screen reader hears it); the listbox is drawn only while it has options. -->
            <span class="pd-pal-none" data-testid="palette-empty">{{ emptyText() }}</span>
          }
        </div>
        @if (store.rows().length) {
        <ul class="pd-pal-list" role="listbox" [attr.aria-label]="t.results" [id]="listId" data-testid="palette-list">
          @for (row of store.rows(); track row.key; let i = $index) {
            @if (row.heading) {
              <li class="pd-pal-group" role="presentation" aria-hidden="true">{{ row.heading }}</li>
            }
            <!-- eslint-disable-next-line @angular-eslint/template/click-events-have-key-events, @angular-eslint/template/interactive-supports-focus -- options are chosen with the keyboard from the input (aria-activedescendant); the click is the pointer path -->
            <li
              role="option"
              class="pd-pal-opt"
              [id]="optionId(i)"
              [class.pd-active]="i === store.active()"
              [class.pd-disabled]="!!row.disabled"
              [attr.aria-selected]="i === store.active()"
              [attr.aria-disabled]="row.disabled ? 'true' : null"
              [attr.title]="row.disabled ?? null"
              [attr.data-key]="row.key"
              (mousemove)="store.setActive(i)"
              (click)="accept(i)"
            >
              @if (row.icon) {
                <pd-icon class="pd-pal-icon" [name]="row.icon" />
              }
              @if (row.category) {
                <span class="pd-pal-cat">{{ row.category }}:</span>
              }
              <span class="pd-pal-label">@for (p of parts(row); track $index) {@if (p.mark) {<mark>{{ p.text }}</mark>} @else {{{ p.text }}}}</span>
              @if (row.detail) {
                @if (row.detailIsPath) {
                  <pd-path-text class="pd-pal-detail" [value]="row.detail" [plain]="true" />
                } @else {
                  <span class="pd-pal-detail" [attr.title]="row.detail">{{ row.detail }}</span>
                }
              }
              @if (row.keybinding) {
                <kbd class="pd-pal-kbd">{{ row.keybinding }}</kbd>
              }
            </li>
          }
        </ul>
        }
        <div class="pd-pal-hints" aria-hidden="true">
          @for (p of store.prefixes(); track p.prefix) {
            <span class="pd-pal-hint" [class.pd-active]="p.active" data-testid="palette-hint"><kbd>{{ p.prefix }}</kbd> {{ p.title }}</span>
          }
        </div>
      </div>
    }
  `,
  styles: `
    :host { display: contents; }
    .pd-pal-backdrop { position: fixed; inset: 0; z-index: var(--pd-z-overlay); background: var(--pd-scrim); }
    .pd-pal { position: fixed; z-index: calc(var(--pd-z-overlay) + 1); top: 64px; left: 50%; transform: translateX(-50%); width: min(600px, calc(100vw - 32px)); display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-2); border-radius: var(--pd-radius-popover); background: var(--pd-surface-2); border: 1px solid var(--pd-border-strong); box-shadow: var(--pd-shadow-3); color: var(--pd-text); font-size: var(--pd-text-body); }
    .pd-pal-input { height: 32px; min-width: 0; padding: 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-input); background: var(--pd-bg); color: var(--pd-text); font: inherit; }
    .pd-pal-input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -1px; }
    .pd-pal-status { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 0; padding: 0 var(--pd-space-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-pal-error { color: var(--pd-danger); min-width: 0; overflow-wrap: anywhere; }
    .pd-pal-retry { padding: 0 var(--pd-space-2); height: 22px; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; cursor: pointer; }
    .pd-pal-retry:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-pal-list { list-style: none; margin: 0; padding: 0; max-height: min(400px, 60vh); overflow-y: auto; overflow-x: hidden; display: flex; flex-direction: column; }
    .pd-pal-opt { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 30px; min-width: 0; padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); color: var(--pd-text); cursor: pointer; }
    .pd-pal-opt.pd-active { background: var(--pd-hover); box-shadow: inset 2px 0 0 var(--pd-accent); }
    .pd-pal-opt.pd-disabled { color: var(--pd-text-subtle); cursor: default; }
    .pd-pal-icon { flex: none; }
    .pd-pal-cat { flex: none; color: var(--pd-text-muted); }
    .pd-pal-label { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-pal-label mark { background: none; color: var(--pd-text); font-weight: var(--pd-weight-semibold); text-decoration: underline 2px var(--pd-accent-fill); text-underline-offset: 3px; }
    .pd-pal-detail { flex: 0 1 40%; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-pal-group { padding: var(--pd-space-2) var(--pd-space-3) var(--pd-space-1); color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-semibold); }
    .pd-pal-kbd, .pd-pal-hint kbd { flex: none; font-family: var(--pd-font-sans); font-variant-numeric: tabular-nums; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-pal-none { padding: var(--pd-space-2) 0; color: var(--pd-text-muted); font-size: var(--pd-text-body); }
    .pd-pal-hint.pd-active, .pd-pal-hint.pd-active kbd { color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .pd-pal-hints { display: flex; flex-wrap: wrap; gap: var(--pd-space-3); padding: var(--pd-space-1) var(--pd-space-2) 0; border-top: 1px solid var(--pd-hairline); color: var(--pd-text-muted); font-size: var(--pd-text-label); }
  `,
})
export class PdCommandPalette {
  protected readonly store = inject(PaletteStore);
  private readonly injector = inject(Injector);
  private readonly doc = inject(DOCUMENT);
  private readonly input = viewChild<ElementRef<HTMLInputElement>>('input');
  protected readonly listId = `pd-palette-list-${nextId++}`;
  protected readonly t = {
    dialog: $localize`:@@pd.wb.palette.dialog:Command palette`,
    input: $localize`:@@pd.wb.palette.input:Type a command, or a prefix to search`,
    results: $localize`:@@pd.wb.palette.results:Results`,
    empty: $localize`:@@pd.wb.palette.empty:Nothing matches`,
    searching: $localize`:@@pd.wb.palette.searching:Searching…`,
    retry: $localize`:@@pd.wb.palette.retry:Try again`,
  };
  protected readonly activeId = computed(() => (this.store.rows().length ? this.optionId(this.store.active()) : null));
  protected readonly didYouMean = computed(() => $localize`:@@pd.wb.palette.did-you-mean:Nothing matches “${this.store.searched()}:query:”. Did you mean:`);
  /** The empty list says what was searched, and where (review P2-9). */
  protected readonly emptyText = computed(() => {
    const searched = this.store.searched();
    const provider = this.store.provider();
    if (!searched) return this.t.empty;
    return provider
      ? $localize`:@@pd.wb.palette.empty-in:Nothing in ${provider.title.text}:source: matches “${searched}:query:”`
      : $localize`:@@pd.wb.palette.empty-query:No command matches “${searched}:query:”`;
  });
  private returnFocus: HTMLElement | null = null;
  /** The input has the focus (it is drawn and focused a moment after the palette opens). */
  protected inputFocused = false;
  private readonly focused = signal(false);
  /** Enter was pressed before the input had the focus: it accepts once the typed query's rows are in. */
  private pendingEnter = false;

  constructor() {
    let wasOpen = false;
    effect(() => {
      const ready = this.store.isOpen() && this.focused() && !this.store.isLoading();
      untracked(() => {
        if (!ready || !this.pendingEnter) return;
        this.pendingEnter = false;
        void this.accept(this.store.active());
      });
    });
    // effect-state: focus and the early-key flags live outside the signal graph (the DOM's focus); this resets them when the palette closes.
    // Focus is outside the signal graph: remember where it was when the palette opens, and put the caret in the input.
    effect(() => {
      const open = this.store.isOpen();
      untracked(() => {
        if (open && !wasOpen) {
          this.returnFocus = this.doc.activeElement as HTMLElement | null;
          afterNextRender(
            () => {
              const input = this.input()?.nativeElement;
              if (!input) return;
              input.focus();
              this.markFocused(input);
            },
            { injector: this.injector },
          );
        }
        if (!open && wasOpen) {
          this.pendingEnter = false;
          this.inputFocused = false;
          this.focused.set(false);
          this.returnFocus?.focus?.();
          this.returnFocus = null;
        }
      });
      wasOpen = open;
    });
  }

  protected onFocus(e: Event): void {
    this.markFocused(e.target as HTMLInputElement);
  }

  private markFocused(input: HTMLInputElement): void {
    this.inputFocused = true;
    this.focused.set(true);
    // Keys kept for the input (above) may be newer than what was drawn: the next key must land on the whole query.
    if (input.value !== this.store.query()) input.value = this.store.query();
    input.setSelectionRange(input.value.length, input.value.length);
  }

  protected onBlur(): void {
    this.inputFocused = false;
    this.focused.set(false);
  }

  /**
   * Keys typed between the shortcut and the input getting the focus (a render later) are not lost: a printable key or
   * Backspace edits the query, Enter waits for the input and the rows of what was typed (it must not run a row of a list nobody has looked at), Escape closes.
   */
  protected onEarlyKey(e: KeyboardEvent): void {
    if (!this.store.isOpen() || this.inputFocused || e.defaultPrevented || e.isComposing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      this.close();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      this.pendingEnter = true;
    } else if (e.key === 'Backspace') {
      e.preventDefault();
      this.store.setQuery(this.store.query().slice(0, -1));
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      this.store.setQuery(this.store.query() + e.key);
    }
  }

  protected optionId(i: number): string {
    return `${this.listId}-${i}`;
  }

  protected parts(row: PaletteRow): TitlePart[] {
    return titleParts(row.label, row.marks);
  }

  protected onKey(e: KeyboardEvent): void {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        this.store.move(1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        this.store.move(-1);
        break;
      case 'PageDown':
        e.preventDefault();
        this.store.setActive(this.store.active() + 10);
        break;
      case 'PageUp':
        e.preventDefault();
        this.store.setActive(this.store.active() - 10);
        break;
      case 'Enter':
        e.preventDefault();
        void this.accept(this.store.active());
        break;
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();
        this.close();
        break;
      case 'Tab':
        // The palette is modal: focus stays in its input.
        e.preventDefault();
        break;
    }
  }

  protected close(): void {
    this.store.close();
  }

  protected accept(index: number): Promise<boolean> {
    // Whatever ran decides focus (a view takes it); do not pull it back to where the palette was opened.
    this.returnFocus = null;
    return this.store.accept(index);
  }
}
