import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, Injector, input, LOCALE_ID, output, TemplateRef, ViewEncapsulation } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatProgressBar } from '@angular/material/progress-bar';
import { MatButton } from '@angular/material/button';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import type { Progress, WidgetState } from '../generated/widgets';
import { relativeAgo } from './time';
import { PD_WIDGET_LOADING, type PdWidgetContext } from './widget';

let nextId = 0;

/** The head's right-hand text: stale readings carry a clock; `full` is the whole wording for the tooltip and the accessible name. */
interface Meta {
  readonly text: string;
  readonly full: string;
  readonly stale: boolean;
}

/**
 * One widget's frame (ADR 0035 §4, design guide §8): one token step (`--pd-surface-2`) with no border, a head (icon, title,
 * optional meta) and a body by state. `state` null or undefined is loading: the app's ready template is drawn with no value and
 * its `pdSkeleton` parts as blocks, so the loading height is the ready height. Ready draws the template; working draws the
 * phase, a progress bar (indeterminate for a count) and the last value; empty and error draw their title, detail and next action.
 *
 * A busy source is working or a stale ready, never an error; a frame or row that opens a page is one real `button`.
 */
@Component({
  selector: 'pd-widget-frame',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [NgTemplateOutlet, MatIcon, MatProgressBar, MatButton, PdGlyph, PdStatusGlyph],
  host: { class: 'pd-widget-frame', '[class.pd-widget-frame--card]': "variant() === 'card'", '[style.grid-column]': '"span " + span()' },
  template: `
    <section class="pd-w" [class.pd-w--loading]="loading()" [attr.aria-labelledby]="titleId" [attr.aria-busy]="loading() ? 'true' : null">
      <header class="pd-w-head">
        <mat-icon class="pd-w-icon" [svgIcon]="icon()" />
        <span class="pd-w-title" [id]="titleId">{{ title() }}</span>
        @if (meta(); as m) {
          @if (m.stale) {
            <span class="pd-w-meta pd-w-meta--stale" [attr.title]="m.full" [attr.aria-label]="m.full" role="img"
              ><pd-glyph name="clock" [size]="14" />{{ m.text }}</span
            >
          } @else {
            <span class="pd-w-meta">{{ m.text }}</span>
          }
        }
      </header>
      @let s = state();
      @if (s?.state === 'empty') {
        @if (s.calm && s.next) {
          <button type="button" class="pd-w-calm" [attr.aria-label]="s.title + ', ' + s.detail + '. ' + s.next.label" (click)="action.emit(s.next.action)">
            <pd-glyph class="pd-w-ok" name="check-circle" [size]="16" />
            <span class="pd-w-calm-text"><span>{{ s.title }}</span><span class="pd-w-subtle">&nbsp;· {{ s.detail }}</span></span>
            <pd-glyph class="pd-w-chev" name="chevron-right" [size]="14" />
          </button>
        } @else {
          <div class="pd-w-st" role="status">
            <pd-status-glyph status="info" [size]="16" />
            <span class="pd-w-t">{{ s.title }}</span>
            <span class="pd-w-d">{{ s.detail }}</span>
            @if (s.next; as n) {
              <button matButton="text" type="button" class="pd-w-act" (click)="action.emit(n.action)">{{ n.label }}</button>
            }
          </div>
        }
      } @else if (s?.state === 'error') {
        <div class="pd-w-st pd-w-st--error" role="alert">
          <pd-status-glyph class="pd-w-bad" status="error" [size]="16" />
          <span class="pd-w-t">{{ s.title }}</span>
          @if (s.cause) {
            <span class="pd-w-d">{{ s.cause }}</span>
          }
          @if (s.next; as n) {
            <button matButton="text" type="button" class="pd-w-act" (click)="action.emit(n.action)"><pd-glyph name="refresh" [size]="16" />{{ n.label }}</button>
          }
        </div>
      } @else if (s?.state === 'working') {
        <div class="pd-w-st">
          <pd-glyph name="refresh" [size]="16" />
          <span class="pd-w-t">{{ s.title }}</span>
          @let detail = detailOf(s.phase, s.progress, locale);
          @if (detail) {
            <span class="pd-w-d">{{ detail }}</span>
          }
          <mat-progress-bar class="pd-w-prog" [mode]="percent(s.progress) === null ? 'indeterminate' : 'determinate'" [value]="percent(s.progress) ?? 0" [attr.aria-label]="s.title" />
          @if (s.cancel; as c) {
            <button matButton="text" type="button" class="pd-w-act" (click)="action.emit(c.action)">{{ c.label }}</button>
          }
        </div>
        @if (s.last !== null && template(); as tpl) {
          <ng-container *ngTemplateOutlet="tpl; context: { $implicit: s.last, loading: false, stale: false }; injector: readyInjector" />
        }
      } @else if (template(); as tpl) {
        <ng-container *ngTemplateOutlet="tpl; context: readyContext(); injector: readyInjector" />
      }
    </section>
  `,
  styles: `
    .pd-widget-frame { display: block; min-width: 0; }
    .pd-w { display: grid; grid-template-columns: minmax(0, 1fr); gap: 6px; align-content: start; min-width: 0; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); color: var(--pd-text); }
    .pd-widget-frame--card { height: 100%; }
    /* The bottom padding clears the grid's corner resize grip (24 px at 4 px from the corner) so it never covers the last line. */
    .pd-widget-frame--card .pd-w { box-sizing: border-box; height: 100%; padding: var(--pd-space-3) var(--pd-space-3) calc(var(--pd-space-3) + var(--pd-widget-grip-clear, 16px)); overflow: hidden; align-content: start; }
    /* An inert body (the grid while editing) reads as inactive: a filled primary or accent text button loses its emphasis, so the page's own primary action stays the only one. */
    .pd-widget-frame[inert] .mat-mdc-unelevated-button:not(:disabled) { background-color: transparent; color: var(--pd-text-muted); }
    .pd-widget-frame[inert] .mat-mdc-button:not(:disabled), .pd-widget-frame[inert] .pd-w-act { color: var(--pd-text-muted); }
    .pd-w-head { display: flex; align-items: center; gap: 6px; height: 16px; font-size: var(--pd-text-label); line-height: var(--pd-lh-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); white-space: nowrap; }
    .pd-w-icon.mat-icon { width: 14px; height: 14px; flex: none; }
    .pd-w-title { overflow: hidden; text-overflow: ellipsis; min-width: 0; }
    .pd-w-meta { flex: none; margin-left: auto; font-weight: var(--pd-weight-regular); color: var(--pd-text-subtle); overflow: hidden; text-overflow: ellipsis; }
    .pd-w-meta--stale { display: inline-flex; align-items: center; gap: var(--pd-space-1); color: var(--pd-text-muted); }
    .pd-w-subtle { color: var(--pd-text-subtle); }
    .pd-w .pd-sk { color: transparent !important; border-radius: var(--pd-radius-tag); background: linear-gradient(90deg, var(--pd-skeleton-raised) 0 35%, var(--pd-skeleton-raised-shine) 50%, var(--pd-skeleton-raised) 65% 100%); background-size: 300% 100%; animation: pd-w-shimmer 1.6s linear infinite; }
    .pd-w .pd-sk * { visibility: hidden; }
    @keyframes pd-w-shimmer { from { background-position: 100% 0; } to { background-position: -100% 0; } }
    @media (prefers-reduced-motion: reduce) { :root:not([data-motion='full']) .pd-w .pd-sk { animation: none; background: var(--pd-skeleton-raised); } }
    :root[data-motion='reduce'] .pd-w .pd-sk { animation: none; background: var(--pd-skeleton-raised); }
    .pd-w-st { display: grid; grid-template-columns: 16px minmax(0, 1fr); gap: var(--pd-space-0) var(--pd-space-2); align-items: start; padding: var(--pd-space-0) 0; }
    .pd-w-st > pd-glyph, .pd-w-st > pd-status-glyph { margin-top: var(--pd-space-0); color: var(--pd-text-muted); }
    .pd-w-st .pd-w-bad { color: var(--pd-danger); }
    .pd-w-t { font-weight: var(--pd-weight-semibold); }
    .pd-w-d { grid-column: 2; font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-w-act, .pd-w-st .pd-w-prog { grid-column: 2; }
    .pd-w-act { justify-self: start; margin: var(--pd-space-1) 0 0 calc(-1 * var(--pd-space-2)); --mat-button-text-horizontal-padding: var(--pd-space-2); padding-inline: var(--pd-space-2); --mat-button-text-label-text-color: var(--pd-accent); color: var(--pd-accent); }
    .pd-w-prog { height: 4px; margin-top: var(--pd-space-1); border-radius: 2px; }
    .pd-w-calm { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 28px; width: calc(100% + 2 * var(--pd-space-2)); margin: 0 calc(-1 * var(--pd-space-2)); padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-control); background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
    .pd-w-calm:hover { background: var(--pd-hover); }
    .pd-w-calm:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-w-calm-text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-w-ok { color: var(--pd-success); }
    .pd-w-chev { color: var(--pd-text-muted); }
    /* High contrast: the fill step is not enough, so the frame gets the strong border colour as an outline. */
    @media (prefers-contrast: more) { :root:not([data-contrast='off']) .pd-w { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; } }
    :root[data-contrast='more'] .pd-w { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; }
  `,
})
export class PdWidgetFrame<V = unknown, A = unknown> {
  private readonly injector = inject(Injector);

  /** The kind's title (the head). */
  readonly title = input.required<string>();
  /** The kind's registered icon name (`mat-icon` `svgIcon`, for example `pd:bell`). */
  readonly icon = input.required<string>();
  /** The widget's state; `null` or `undefined` is loading. */
  readonly state = input<WidgetState<V, A> | null | undefined>(null);
  /** The app's template for the kind (`PdWidget.template`). */
  readonly template = input<TemplateRef<PdWidgetContext<V>> | null>(null);
  /** `tile`: a popup tile that sizes to its content. `card`: fills its grid cell (the widget grid, ADR 0044), head and body clipped to the cell. */
  readonly variant = input<'tile' | 'card'>('tile');
  /** Columns spanned in the area's grid. */
  readonly span = input(2);
  /** A live kind says "Paused at 14:28" when stale; the others say "Updated 4 min ago". */
  readonly live = input(false);
  /**
   * The clock (ms since the epoch) for "Updated 4 min ago". Nothing here ticks: the text is computed when `state` or `now` changes, so an
   * app that keeps a popup open passes a `now` that advances (a signal updated every minute). Null reads the system clock when the state changes.
   */
  readonly now = input<number | null>(null);
  /** Right-aligned text of the head for a fresh reading ("3 volumes"). */
  readonly metaText = input<string | null>(null);
  /** The app's action chosen on the empty, error or working state. */
  readonly action = output<A>();

  protected readonly titleId = `pd-widget-${nextId++}`;
  protected readonly loading = computed(() => this.state() == null);
  protected readonly readyInjector = Injector.create({ parent: this.injector, providers: [{ provide: PD_WIDGET_LOADING, useValue: this.loading }] });

  protected readonly readyContext = computed<PdWidgetContext<V>>(() => {
    const s = this.state();
    return s?.state === 'ready' ? { $implicit: s.value, loading: false, stale: s.staleSince !== null } : { $implicit: null, loading: true, stale: false };
  });

  protected readonly meta = computed<Meta | null>(() => {
    const s = this.state();
    if (s?.state !== 'ready') return null;
    if (s.staleSince !== null) {
      const ago = relativeAgo(this.now() ?? Date.now(), s.staleSince);
      const full = this.live() ? `Paused at ${clock(s.staleSince)}` : `Updated ${ago}`;
      const short = this.live() ? clock(s.staleSince) : ago.replace(/ ago$/, '');
      return { text: this.span() === 1 ? short : full, full, stale: true };
    }
    const m = this.metaText();
    return m ? { text: m, full: m, stale: false } : null;
  });

  protected readonly locale = inject(LOCALE_ID);
  protected detailOf = detailOf;
  protected percent = percent;
}

/** The share of a fraction (0 to 100), or null for a count or unknown progress (an indeterminate bar). */
function percent(p: Progress): number | null {
  return p !== 'unknown' && 'fraction' in p ? (p.fraction.total > 0 ? Math.min(100, (p.fraction.done / p.fraction.total) * 100) : 0) : null;
}

/** The working state's progress as words: a growing count when the total is unknown ("12,400 events so far"), else "2 of 3". */
export function progressText(p: Progress, locale: string): string {
  if (p === 'unknown') return '';
  const n = new Intl.NumberFormat(locale);
  if ('count' in p) return `${n.format(p.count.done)} ${p.count.unit} so far`.replace('  ', ' ').trim();
  return `${n.format(p.fraction.done)} of ${n.format(p.fraction.total)} ${p.fraction.unit}`.trim();
}

/** The working line under the title: the phase and the progress words, joined; empty when there is neither. */
function detailOf(phase: string, p: Progress, locale: string): string {
  return [phase, progressText(p, locale)].filter((x) => x !== '').join(' · ');
}

function clock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}
