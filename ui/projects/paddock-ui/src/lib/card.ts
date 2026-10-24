import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

let nextId = 0;

/**
 * Tile with one hero: header (icon + title + status), the hero (default slot), a sparkline slot and a caption.
 * Whole tile is a button when `interactive` (chevron on hover, focus ring). Surface-1, hairline border, radius 10.
 *
 * Slots: `[slot=icon]`, `[slot=status]`, default (hero), `[slot=sparkline]`, `[slot=caption]`.
 */
@Component({
  selector: 'pd-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.pd-card--interactive]': 'interactive()',
    '[class.pd-card--flat]': 'flat()',
    '[attr.role]': 'interactive() ? "button" : "group"',
    '[attr.tabindex]': 'interactive() ? 0 : null',
    '[attr.aria-labelledby]': 'interactive() ? null : titleId',
    '[attr.aria-label]': 'interactive() ? heading() : null',
    '(click)': 'activate()',
    '(keydown.enter)': 'activate($event)',
    '(keydown.space)': 'activate($event)',
  },
  template: `
    <header class="pd-head">
      <span class="pd-icon"><ng-content select="[slot=icon]" /></span>
      <h3 class="pd-title" [id]="titleId">{{ heading() }}</h3>
      <span class="pd-status"><ng-content select="[slot=status]" /></span>
    </header>
    <div class="pd-hero"><ng-content /></div>
    <div class="pd-spark"><ng-content select="[slot=sparkline]" /></div>
    <div class="pd-caption"><ng-content select="[slot=caption]" /></div>
    @if (interactive()) {
      <svg class="pd-chevron" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
        <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    }
  `,
  styles: `
    :host {
      position: relative;
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-2);
      min-width: 0;
      padding: var(--pd-card-padding);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      color: var(--pd-text);
    }
    /* Inside another bordered surface (tray popover, dialog): a background step instead of a nested border. */
    :host(.pd-card--flat) { background: var(--pd-surface-2); border-color: transparent; }
    :host(.pd-card--interactive) { cursor: pointer; transition: background var(--pd-duration-micro) var(--pd-ease-enter); }
    :host(.pd-card--interactive:hover) { background: var(--pd-surface-2); }
    :host(.pd-card--interactive:active) { background: var(--pd-pressed); }
    .pd-head { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 20px; }
    .pd-icon { display: inline-flex; color: var(--pd-text-muted); }
    .pd-icon:empty { display: none; }
    .pd-title { margin: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--pd-text-body); line-height: var(--pd-lh-body); font-weight: var(--pd-weight-semibold); }
    .pd-status { display: inline-flex; align-items: center; }
    .pd-spark:empty, .pd-caption:empty { display: none; }
    .pd-caption { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .pd-chevron { position: absolute; right: var(--pd-space-3); bottom: var(--pd-space-3); color: var(--pd-text-muted); opacity: 0; transition: opacity var(--pd-duration-micro) var(--pd-ease-enter); }
    :host(.pd-card--interactive:hover) .pd-chevron, :host(.pd-card--interactive:focus-visible) .pd-chevron { opacity: 1; }
    :host(:focus-visible) { border-radius: var(--pd-radius-card); }
  `,
})
export class SuCard {
  readonly heading = input.required<string>();
  /** The whole tile navigates: it becomes a button. */
  readonly interactive = input<boolean>(false);
  /** Borderless, surface-2 fill: use inside a surface that already has a border (no nested bordered cards). */
  readonly flat = input<boolean>(false);
  readonly activated = output<void>();
  protected readonly titleId = `pd-card-title-${nextId++}`;

  protected activate(event?: Event): void {
    if (!this.interactive()) return;
    event?.preventDefault();
    this.activated.emit();
  }
}
