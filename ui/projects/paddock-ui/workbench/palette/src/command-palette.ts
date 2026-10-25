import { DOCUMENT } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, computed, effect, ElementRef, inject, Injector, untracked, viewChild } from '@angular/core';
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
          aria-expanded="true"
          [attr.aria-controls]="listId"
          [attr.aria-activedescendant]="activeId()"
          [placeholder]="store.placeholder()"
          autocomplete="off"
          spellcheck="false"
          data-testid="palette-input"
          [value]="store.query()"
          (input)="store.setQuery($any($event.target).value)"
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
          }
        </div>
        <ul class="pd-pal-list" role="listbox" [attr.aria-label]="t.results" [id]="listId" data-testid="palette-list">
          @for (row of store.rows(); track row.key; let i = $index) {
            @if (row.heading) {
              <li class="pd-pal-group" role="presentation" aria-hidden="true">{{ row.heading }}</li>
            }
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
          } @empty {
            @if (!store.isLoading()) {
              <li class="pd-pal-none" role="presentation" data-testid="palette-empty">{{ emptyText() }}</li>
            }
          }
        </ul>
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
    .pd-pal-none { padding: var(--pd-space-3); color: var(--pd-text-muted); }
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

  constructor() {
    let wasOpen = false;
    // Focus is outside the signal graph: remember where it was when the palette opens, and put the caret in the input.
    effect(() => {
      const open = this.store.isOpen();
      untracked(() => {
        if (open && !wasOpen) {
          this.returnFocus = this.doc.activeElement as HTMLElement | null;
          afterNextRender(() => this.input()?.nativeElement.focus(), { injector: this.injector });
        }
        if (!open && wasOpen) {
          this.returnFocus?.focus?.();
          this.returnFocus = null;
        }
      });
      wasOpen = open;
    });
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
