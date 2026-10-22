import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

let nextId = 0;

/**
 * Tile with one hero: header (icon + title + status), the hero (default slot), a sparkline slot and a caption.
 * Whole tile is a button when `interactive` (chevron on hover, focus ring). Surface-1, hairline border, radius 10.
 *
 * Slots: `[slot=icon]`, `[slot=status]`, default (hero), `[slot=sparkline]`, `[slot=caption]`.
 */
@Component({
  selector: 'su-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.su-card--interactive]': 'interactive()',
    '[class.su-card--flat]': 'flat()',
    '[attr.role]': 'interactive() ? "button" : "group"',
    '[attr.tabindex]': 'interactive() ? 0 : null',
    '[attr.aria-labelledby]': 'interactive() ? null : titleId',
    '[attr.aria-label]': 'interactive() ? heading() : null',
    '(click)': 'activate()',
    '(keydown.enter)': 'activate($event)',
    '(keydown.space)': 'activate($event)',
  },
  template: `
    <header class="head">
      <span class="icon"><ng-content select="[slot=icon]" /></span>
      <h3 class="title" [id]="titleId">{{ heading() }}</h3>
      <span class="status"><ng-content select="[slot=status]" /></span>
    </header>
    <div class="hero"><ng-content /></div>
    <div class="spark"><ng-content select="[slot=sparkline]" /></div>
    <div class="caption"><ng-content select="[slot=caption]" /></div>
    @if (interactive()) {
      <svg class="chevron" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
        <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    }
  `,
  styles: `
    :host {
      position: relative;
      display: flex;
      flex-direction: column;
      gap: var(--su-space-2);
      min-width: 0;
      padding: var(--su-card-padding);
      background: var(--su-surface-1);
      border: 1px solid var(--su-hairline);
      border-radius: var(--su-radius-card);
      color: var(--su-text);
    }
    /* Inside another bordered surface (tray popover, dialog): a background step instead of a nested border. */
    :host(.su-card--flat) { background: var(--su-surface-2); border-color: transparent; }
    :host(.su-card--interactive) { cursor: pointer; transition: background var(--su-duration-micro) var(--su-ease-enter); }
    :host(.su-card--interactive:hover) { background: var(--su-surface-2); }
    :host(.su-card--interactive:active) { background: var(--su-pressed); }
    .head { display: flex; align-items: center; gap: var(--su-space-2); min-height: 20px; }
    .icon { display: inline-flex; color: var(--su-text-muted); }
    .icon:empty { display: none; }
    .title { margin: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--su-text-body); line-height: var(--su-lh-body); font-weight: var(--su-weight-semibold); }
    .status { display: inline-flex; align-items: center; }
    .spark:empty, .caption:empty { display: none; }
    .caption { font-size: var(--su-text-label); line-height: var(--su-lh-label); color: var(--su-text-muted); font-variant-numeric: tabular-nums; }
    .chevron { position: absolute; right: var(--su-space-3); bottom: var(--su-space-3); color: var(--su-text-muted); opacity: 0; transition: opacity var(--su-duration-micro) var(--su-ease-enter); }
    :host(.su-card--interactive:hover) .chevron, :host(.su-card--interactive:focus-visible) .chevron { opacity: 1; }
    :host(:focus-visible) { border-radius: var(--su-radius-card); }
  `,
})
export class SuCard {
  readonly heading = input.required<string>();
  /** The whole tile navigates: it becomes a button. */
  readonly interactive = input<boolean>(false);
  /** Borderless, surface-2 fill: use inside a surface that already has a border (no nested bordered cards). */
  readonly flat = input<boolean>(false);
  readonly activated = output<void>();
  protected readonly titleId = `su-card-title-${nextId++}`;

  protected activate(event?: Event): void {
    if (!this.interactive()) return;
    event?.preventDefault();
    this.activated.emit();
  }
}
