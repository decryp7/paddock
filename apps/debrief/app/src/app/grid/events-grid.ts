import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, linkedSignal, signal, untracked, viewChild } from '@angular/core';
import { ErrorDetails } from '../ui/error-details';
import { RecordView } from '../core/api.types';
import { DebriefBackend } from '../core/backend';
import { formatCount, formatDateTime } from '../core/format';
import { describeError, Logger } from '../core/logger';
import { LatestRequest } from '../shared/latest-request';
import { QueryStore } from '../core/workspace/query.store';
import { SelectionStore } from '../core/workspace/selection.store';
import { UiPrefsStore } from '../core/workspace/ui-prefs.store';
import { WorkspaceDataStore } from '../core/workspace/workspace-data.store';
import { LiveTail } from '../workspace/live-tail';
import { LiveFeed } from '../live/live-feed';
import { NewEventsPill } from '../live/new-events-pill';
import { VirtualList } from '../workspace/virtual-list';
import { ClickOutside } from '../shared/click-outside';
import { LEVEL_CODES, LEVEL_VARS } from '../shared/colors';
import { Icon } from '../shared/icon';
import { SettingsStore } from '../settings/settings.store';
import { ThemeService } from '../core/workspace/theme.service';
import { px } from '../theme/theme-tokens';
import { RecordActions } from '../context/record-actions';
import { FieldFormatters } from '../extensions/field-formatters';
import { MarkerIcon } from '../shared/marker-icon';
import { HighlightRules } from '../projects/bookmarks/highlight-rules.store';
import { NotesStore } from '../notes/notes.store';
import { firstLine } from '../notes/notes-model';
import { GridSelection } from './grid-selection';
import { contains, EMPTY_SELECTION, extendTo, indices, RowSelection, shift, single, toggle, withFocus } from './row-selection';
import { makeGeometry, navigate, rowAtTop, rowY, scrollToTop, topForRow, topToScroll, visibleWindow, wheelToPixels, clamp } from './scroll-map';
import { NextSteps } from '../ui/next-steps';
import { SourceColors, SourceMark } from '../sources/source-colors.store';
import { stripeBackground } from '../sources/source-colors';
import { LiveArrival, PagedRowsStore } from './paged-rows.store';
import { FollowEvent, FollowState, followStep } from './follow-reducer';
import { classifyScroll } from './scroll-classify';
import { writeScrollTop } from './scroll-writer';
import { LiveFollow } from '../live/live-follow.store';
import { writeEventDrag } from '../core/event-drag';

export { SCROLL_IDLE_MS } from './paged-rows.store';

/** What a row's note glyph says: the first note's first line, and how many more there are. */
export function noteLabel(notes: readonly { text: string }[] | undefined): string | null {
  const [first] = notes ?? [];
  if (!first) return null;
  const line = firstLine(first.text, 80) || $localize`:@@grid.note:Note`;
  return notes!.length > 1 ? $localize`:@@grid.notesMore:${line}:note: (+${notes!.length - 1}:more: more)` : line;
}

/** Row heights for the two grid densities (px). */
/** Row height when the theme gives none (the theme's `--row-h` per density wins). */
const FALLBACK_ROW_H = 24;
const HEADER_H = 28;
const OVERSCAN = 8;
/** At most this many selected lines go to actions on several records (e.g. "Infer pattern from lines…"). */
const MAX_ACTION_RECORDS = 200;

let gridSeq = 0;

/** Anchor requests survive the grid being destroyed (tab switch); only act on new ones (per store). */
const handledAnchorNonce = new WeakMap<object, number>();

/** "OrderService · WEB01 · 3312" as the name and the muted rest (host, process id). */
export function splitInstance(group: string): { instName: string; instRest: string } {
  const i = group.indexOf(' · ');
  return i < 0 ? { instName: group, instRest: '' } : { instName: group.slice(0, i), instRest: group.slice(i + 3) };
}

/** What one rendered row shows: everything the template needs, computed once per change instead of per binding. */
export interface RowVm {
  kind: 'row';
  index: number;
  y: number;
  row: RecordView;
  /** Element id (the grid names the focused row with aria-activedescendant). */
  id: string;
  selected: boolean;
  /** Colour of the first highlight rule that matches, if any. */
  highlight: string | null;
  /** Source / instance colour stripe. */
  mark: SourceMark | null;
  stripe: string | null;
  marker: { kind: string; color: string | null; label: string } | null;
  /** The event has notes: what the glyph says (the first note's first line), else null. */
  note: string | null;
  time: string;
  /** The event has no timestamp: it is in the grid but not on the timeline (the time cell says so). */
  untimed: boolean;
  levelVar: string;
  levelCode: string;
  /** The level's name, for the dot's shape (a cue besides its colour). */
  level: string;
  /** Information, debug and verbose rows show the dot only: the code appears where a level is notable. */
  quietLevel: boolean;
  /** The instance's name, then its host and process id, muted. */
  instName: string;
  instRest: string;
  /** The extra columns' texts, in column order. */
  cells: { column: string; text: string }[];
}

/** A row whose page has not arrived. */
export interface PlaceholderVm {
  kind: 'placeholder';
  index: number;
  y: number;
}

export type GridRowVm = RowVm | PlaceholderVm;

/** How often one jump asks the engine while the query keeps changing under it, before it says so (ADR 0071). */
export const JUMP_ATTEMPTS = 3;

/** One key for a query and a sort order: a jump's answer is for exactly this view. */
function viewKey(query: unknown, descending: boolean): string {
  return JSON.stringify([query, descending]);
}

@Component({
  selector: 'debrief-events-grid',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[style.--row-h.px]': 'rowH()' },
  imports: [Icon, MarkerIcon, ClickOutside, VirtualList, NewEventsPill, NextSteps, ErrorDetails],
  template: `
    <div class="bar">
      <span class="section-title" i18n="@@grid.title">Events</span>
      <span class="total">{{ fmt(total()) }}</span>
      <span class="faint" i18n="@@grid.subtitle">of {{ fmt(data.summary()?.total ?? 0) //i18n(ph="total") }} · sorted by time {{ prefs.descending() ? '↓' : '↑' //i18n(ph="direction") }}</span>
      @if (sel.anchorMissed()) {
        <span class="warn" i18n="@@grid.anchorMissed">That event is hidden by the current filters.</span>
      }
      <span class="spacer"></span>
      <button class="btn ghost" (click)="prefs.toggleSort()" title="Toggle sort order" i18n-title="@@grid.sortToggle">
        <debrief-icon [name]="prefs.descending() ? 'sortDesc' : 'sortAsc'" [size]="14" /> {{ sortLabel() }}
      </button>
      <div class="anchor" (debriefClickOutside)="colsOpen.set(false)">
        <button class="btn ghost" (click)="colsOpen.set(!colsOpen())" [attr.aria-expanded]="colsOpen()" i18n="@@grid.columns"><debrief-icon name="columns" [size]="14" /> Columns</button>
        @if (colsOpen()) {
          <div class="popover cols">
            <input class="input" placeholder="Filter fields" i18n-placeholder="@@grid.filterFields.placeholder" aria-label="Filter fields" i18n-aria-label="@@grid.filterFields.aria" [value]="colFilter()" (input)="colFilter.set($any($event.target).value)" />
            <debrief-virtual-list [items]="colChoices()" [itemHeight]="24" [maxHeight]="300" label="Columns" i18n-label="@@grid.columns.listLabel">
              <ng-template let-f>
                <label class="col-opt">
                  <input type="checkbox" [checked]="prefs.extraColumns().includes(f)" (change)="toggleColumn(f)" />
                  <span class="mono ellipsis">{{ f }}</span>
                </label>
              </ng-template>
            </debrief-virtual-list>
          </div>
        }
      </div>
    </div>

    <debrief-new-events-pill />
    <div
      class="viewport"
      #viewport
      tabindex="0"
      role="grid"
      aria-label="Events" i18n-aria-label="@@grid.aria"
      aria-multiselectable="true"
      [attr.aria-rowcount]="total() + 1"
      [attr.aria-colcount]="colCount()"
      [attr.aria-activedescendant]="activeRowId()"
      (scroll)="onScroll()"
      (wheel)="onWheel($event)"
      (pointerdown)="onPointerDown($event)"
      (touchstart)="onTouchStart($event)"
      (keydown)="onKey($event)"
    >
      <div class="header" role="row" aria-rowindex="1" [style.grid-template-columns]="columns()">
        <span class="hcell" role="columnheader"><span class="sr-only" i18n="@@grid.header.marker">Marker</span></span>
        <span class="hcell" role="columnheader" [attr.aria-sort]="prefs.descending() ? 'descending' : 'ascending'">
          <button class="sort" (click)="prefs.toggleSort()" i18n="@@grid.header.time">Time {{ prefs.descending() ? '↓' : '↑' //i18n(ph="direction") }}</button>
        </span>
        <span class="hcell" role="columnheader" i18n="@@grid.header.level">Level</span>
        <span class="hcell" role="columnheader" i18n="@@grid.header.instance">Instance</span>
        @for (c of prefs.extraColumns(); track c) {
          <span class="hcell extra" role="columnheader"><span class="ellipsis mono">{{ c }}</span><button class="x" [attr.aria-label]="removeColumnLabel(c)" (click)="prefs.removeColumn(c)" [attr.title]="removeColumnLabel(c)">×</button></span>
        }
        <span class="hcell" role="columnheader" i18n="@@grid.header.message">Message</span>
      </div>
      <div class="layer" role="presentation" [style.height.px]="viewportH()">
        @for (v of rowVms(); track v.index) {
          @if (v.kind === 'row') {
            <div
              class="row"
              role="row"
              [id]="v.id"
              [class.focused]="v.id === activeRowId()"
              [attr.aria-rowindex]="v.index + 2"
              [attr.aria-selected]="v.selected"
              [class.sel]="v.selected"
              [class.hl]="!!v.highlight"
              [class.marked]="colors.enabled()"
              [style.--hl]="v.highlight"
              [style.top.px]="v.y"
              [style.grid-template-columns]="columns()"
              draggable="true"
              (dragstart)="onRowDragStart($event, v.row)"
              (mousedown)="onRowMouseDown($event)"
              (click)="onRowClick($event, v.index, v.row)"
              (contextmenu)="openRowMenu($event, v.index, v.row)"
            >
              <span class="mk" role="gridcell">
                @if (v.mark; as m) {
                  <span class="srcmark" role="img" data-testid="source-stripe" [style.background]="v.stripe" [attr.data-pattern]="m.swatch.pattern" [attr.aria-label]="m.label" [title]="m.label"></span>
                }
                @if (v.marker; as mk) { <debrief-marker-icon [kind]="mk.kind" [color]="mk.color" [label]="mk.label" /> }
              </span>
              @if (v.untimed) {
                <span class="mono time untimed" role="gridcell" title="This event has no timestamp, so it is not on the timeline" i18n-title="@@grid.untimedTitle" i18n="@@grid.untimed">no time</span>
              } @else {
                <span class="mono time" role="gridcell">{{ v.time }}</span>
              }
              <span class="lvl" role="gridcell" [attr.data-level]="v.level" [title]="v.levelCode"><span class="dot" [style.background]="v.levelVar"></span><span [class]="v.quietLevel ? 'sr' : 'code'">{{ v.levelCode }}</span></span>
              <span class="inst ellipsis" role="gridcell" [title]="v.row.group"><span class="inst-name">{{ v.instName }}</span>@if (v.instRest) { <span class="inst-rest"> · {{ v.instRest }}</span> }</span>
              @for (c of v.cells; track c.column) {
                <span class="mono extra ellipsis" role="gridcell" [title]="c.text">{{ c.text }}</span>
              }
              <span class="msg ellipsis" role="gridcell">@if (v.note; as n) {<span class="note-glyph" role="img" data-testid="note-glyph" [attr.aria-label]="n" [title]="n"><debrief-icon name="note" [size]="12" /></span>}@if (v.row.searchMatch === 'partial') {<span class="partial" data-testid="partial-match" title="Partial match: the search text appears inside a word" i18n-title="@@grid.partialMatchTitle" i18n="@@grid.partialMatch">partial</span>}{{ v.row.message }}</span>
            </div>
          } @else {
            <div class="row placeholder" role="row" [attr.aria-rowindex]="v.index + 2" [style.top.px]="v.y"><span role="gridcell" class="skeleton" aria-label="Loading" i18n-aria-label="@@grid.loading"></span></div>
          }
        }
        @if (rows.loaded() && total() === 0 && !rows.error()) {
          <div class="none faint" role="row" aria-rowindex="2"><span role="gridcell">{{ emptyMessage() }}<debrief-next-steps /></span></div>
        }
      </div>
      <div aria-hidden="true" [style.height.px]="spacerH()"></div>
    </div>
    @if (pageError(); as err) {
      <div class="err-card" role="alert">
        <debrief-error-details summary="Couldn't load events." i18n-summary="@@grid.error.summary" [detail]="err" (retry)="retry()" />
      </div>
    }
  `,
  styles: `
    :host { flex: 1; min-width: 0; box-sizing: border-box; display: flex; flex-direction: column; background: var(--bg-1); min-height: 0; position: relative; }
    .bar { height: 32px; flex: none; display: flex; align-items: center; gap: 10px; padding: 0 8px 0 14px; border-bottom: 1px solid var(--border); font-size: var(--fs-sm); }
    .total { color: var(--text-2); }
    .warn { color: var(--warn-text); }
    .spacer { flex: 1; }
    .anchor { position: relative; }
    .cols { right: 0; top: 30px; width: 280px; display: flex; flex-direction: column; gap: 6px; }
    .err-card { position: absolute; top: 90px; left: 50%; transform: translateX(-50%); z-index: 5; display: flex; flex-direction: column; align-items: center; gap: 8px; max-width: 80%; padding: 14px 18px; border: 1px solid var(--border-3); border-radius: 8px; background: var(--bg-3); color: var(--text); text-align: center; }
    .col-opt { width: 100%; display: flex; align-items: center; gap: 8px; height: 24px; color: var(--text-2); font-size: var(--fs-sm); }
    .header { position: sticky; top: 0; z-index: 3; height: ${HEADER_H}px; display: grid; align-items: center; column-gap: 10px; padding: 0 14px; font-size: var(--fs-xs); color: var(--muted); border-bottom: 1px solid var(--border); background: var(--bg-2); }
    .hcell { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .sort { border: 0; background: transparent; color: inherit; font-size: var(--fs-xs); padding: 0; text-align: left; cursor: pointer; }
    .sort:hover { color: var(--text); }
    .x { border: 0; background: transparent; color: var(--faint); cursor: pointer; padding: 0 2px; }
    .viewport { flex: 1; min-height: 0; overflow-y: auto; overflow-x: hidden; position: relative; outline: none; }
    .viewport:focus-visible { box-shadow: inset 0 0 0 1px var(--accent); }
    .layer { /* conformance: not-a-header */ position: sticky; top: ${HEADER_H}px; overflow: hidden; }
    .row { position: absolute; left: 0; right: 0; height: var(--row-h, 26px); display: grid; align-items: center; column-gap: 10px; padding: 0 14px; border-bottom: 1px solid var(--grid-line); cursor: default; }
    .row:hover { background: var(--bg-hover); }
    .row.sel { background-color: var(--bg-sel); box-shadow: inset 2px 0 0 var(--accent); }
    /* The row keyboard focus is on (aria-activedescendant), while the grid has focus. */
    .viewport:focus-visible .row.focused { outline: 1px solid var(--accent); outline-offset: -1px; }
    /*
     * Highlight rules: a solid colour bar on the left (readable without relying on the tint) plus a faint tint
     * layered over the hover/selection background, so text contrast and selection styling are kept on both themes.
     */
    .row.hl { background-image: linear-gradient(color-mix(in srgb, var(--hl) var(--row-hl-tint, 12%), transparent) 0 0); }
    .row.hl::before { content: ''; position: absolute; left: 0; top: 0; bottom: 0; width: 4px; background: var(--hl); }
    .row.hl.sel { box-shadow: inset 6px 0 0 var(--accent); }
    /* Source / instance colour: a 4px stripe at the row start (dashed or dotted past four colours), before any highlight bar. */
    .srcmark { position: absolute; left: 0; top: 0; bottom: 0; width: 4px; }
    .srcmark::after { content: ''; position: absolute; inset: 0 -6px 0 0; }
    .row.marked.hl::before { left: 4px; }
    .row.marked.sel { box-shadow: inset 6px 0 0 var(--accent); }
    .row.marked.hl.sel { box-shadow: inset 10px 0 0 var(--accent); }
    .mk { display: flex; justify-content: center; }
    .note-glyph { display: inline-flex; vertical-align: -2px; margin-right: 4px; color: var(--accent-text); }
    .time { font-size: var(--fs-sm); color: var(--muted); white-space: nowrap; }
    .time.untimed { font-style: italic; color: var(--faint); }
    .partial { font-size: var(--fs-xs); color: var(--faint); border: 1px dashed var(--border-2); border-radius: 4px; padding: 0 4px; margin-right: 6px; }
    .lvl { display: flex; align-items: center; gap: 6px; font-size: var(--fs-xs); color: var(--text-2); }
    .extra { font-size: var(--fs-sm); color: var(--text-2); }
    .inst { font-size: var(--fs-sm); color: var(--text-2); font-variant-numeric: tabular-nums; }
    .inst-rest { color: var(--muted); }
    .lvl .code { font-weight: 500; font-variant-numeric: tabular-nums; }
    .lvl .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .extra { color: var(--muted); }
    .msg { color: var(--text); }
    .placeholder { display: flex; }
    .skeleton { width: 60%; height: 8px; border-radius: 4px; background: var(--bg-hover); }
    .none { position: absolute; top: 24px; left: 0; right: 0; text-align: center; }
  `,
})
export class EventsGrid {
  protected readonly data = inject(WorkspaceDataStore);
  private readonly queryStore = inject(QueryStore);
  protected readonly sel = inject(SelectionStore);
  protected readonly prefs = inject(UiPrefsStore);
  private readonly backend = inject(DebriefBackend);
  private readonly logger = inject(Logger);
  private readonly injector = inject(Injector);
  private readonly formatters = inject(FieldFormatters);
  private readonly actions = inject(RecordActions);
  private readonly settings = inject(SettingsStore);
  private readonly live = inject(LiveTail);
  /** Live refreshes: whether to reload the rows in view (following) or keep them ("N new events"). */
  private readonly feed = inject(LiveFeed);
  private readonly follow = inject(LiveFollow);
  private readonly highlight = inject(HighlightRules);
  private readonly notes = inject(NotesStore);
  /** Source / instance colour of each row (View › Source colours). */
  protected readonly colors = inject(SourceColors);
  /** Where the grid publishes its multi-selection for palette commands and other panels. */
  private readonly published = inject(GridSelection);
  private readonly theme = inject(ThemeService);
  /** The theme's row height for the density in effect (the virtual scroll needs it in pixels). */
  protected readonly rowH = computed(() => px(this.theme.tokens()['row-h']) || FALLBACK_ROW_H);
  private readonly viewport = viewChild<ElementRef<HTMLElement>>('viewport');

  /** The rows: paging, eviction, generations and live refreshes (no DOM). */
  protected readonly rows: PagedRowsStore = new PagedRowsStore({
    fetch: (request) => this.backend.queryPage(request),
    onTotal: (total) => this.sel.setMatchTotal(total),
    onLive: (arrival) => this.applyLive(arrival),
    onGrow: ({ newTotal, delta }) => this.applyGrowth(newTotal, delta),
    onPage: () => this.selectPending(),
    centre: (): number => rowAtTop(this.geometry(), this.virtualTop()),
  });

  protected readonly fmt = formatCount;
  protected readonly colsOpen = signal(false);
  protected readonly colFilter = signal('');
  protected readonly total = this.rows.total;
  /** Why loading rows failed: a page or a jump to a record. */
  private readonly jumpError = signal<string | null>(null);
  /** The view (query and order) a jump error is about: a reset to that same view keeps it, another view clears it. */
  private jumpErrorView: string | null = null;
  protected readonly pageError = computed(() => this.rows.error() ?? this.jumpError());
  protected readonly sortLabel = computed(() => (this.prefs.descending() ? $localize`:@@grid.sort.newest:Newest first` : $localize`:@@grid.sort.oldest:Oldest first`));
  protected readonly emptyMessage = computed(() =>
    this.data.summary()?.sources?.length ? $localize`:@@grid.empty.filtered:No events match the current filters.` : $localize`:@@grid.empty.noFiles:No events: nothing is open yet.`,
  );

  protected removeColumnLabel(column: string): string {
    return $localize`:@@grid.removeColumn:Remove column ${column}:column:`;
  }
  protected readonly viewportH = signal(400);
  /**
   * Offset (px) into the true, uncapped content at the top of the rows area. This, not the element's scrollTop,
   * is the source of truth: with tens of millions of rows one scrollbar pixel covers many rows (see scroll-map.ts).
   */
  protected readonly virtualTop = signal(0);

  private queryKey = '';
  /** Selected rows by index (multi-select); its focus is the row the inspector shows (`selected`). */
  protected readonly selection = linkedSignal<RecordView | null, RowSelection>({
    // Something else selected a record (timeline, bookmark, data reset): drop a multi-selection that no longer matches.
    source: () => this.sel.selected(),
    computation: (selected, previous) => {
      const sel = previous?.value ?? EMPTY_SELECTION;
      if (!selected) return EMPTY_SELECTION;
      const focusRow = untracked(() => this.rows.rowAt(sel.focus));
      return focusRow && focusRow.id !== selected.id ? EMPTY_SELECTION : sel;
    },
  });
  /** Row to select once its page arrives (keyboard navigation to a page that is not loaded yet). */
  private pendingSelect: number | null = null;
  /** The jump in flight (ADR 0071): a newer jump, the user's selection or destruction makes it stale. */
  private readonly jumps = new LatestRequest(inject(DestroyRef));
  /** The "is the selected event still in the result" check of the latest reset. */
  private readonly selectedCheck = new LatestRequest(inject(DestroyRef));
  /** The scrollTop we last wrote ourselves; scroll events at that position carry no new information. */
  private writtenScroll = Number.NaN;
  /** The follow and hold state (ADR 0072): `pinned` is stored, not re-derived from the position at each round. */
  private followState: FollowState = { holding: false, pinned: false };
  /** The one scroll write that waits for the render, if any; a newer write replaces it and a user take-over drops it. */
  private pendingWrite: { physical: number } | null = null;
  /** The viewport was measured once (a later change is a resize). */
  private measured = false;
  /** The scrollbar is held (a press on the scroller itself): the view is not re-pinned until it is released. */
  private pointerDown = false;
  /** The origin of the last scroll event that was logged (transitions only). */
  private lastOrigin: 'own' | 'user' | 'spurious' | null = null;
  /** The layout (total rows and viewport height) when the grid last wrote the scroll position: a scroll after a change is the browser's unless an input moved the view. */
  private writtenLayout = '';
  /** The furthest scroll position when the grid last wrote: a scroll after a layout change that moves by that much is the browser's shift. */
  private writtenMax = Number.NaN;
  private layoutKey(el = this.viewport()?.nativeElement): string {
    // The element's own height is read from the DOM: it changes before the resize is reported (`viewportH`).
    return `${this.total()}x${this.viewportH()}x${el?.clientHeight ?? 0}`;
  }
  /** An input event (wheel, pointer, touch, key) reached the grid since the grid last wrote the scroll position. */
  private inputSinceWrite = false;

  protected readonly columns = computed(() => {
    // Narrow windows (H1): the time and source columns give way before the message, which keeps a readable minimum.
    const extra = this.prefs.extraColumns().map(() => 'minmax(90px, 150px)').join(' ');
    return `16px minmax(110px, 150px) 52px minmax(96px, 240px) ${extra} minmax(120px, 1fr)`;
  });

  protected readonly colCount = computed(() => 5 + this.prefs.extraColumns().length);

  protected readonly colChoices = computed(() => {
    const needle = this.colFilter().toLowerCase();
    return ['@source', '@line', '@template', ...this.data.fieldNames()].filter((f) => f.toLowerCase().includes(needle));
  });

  protected readonly geometry = computed(() => makeGeometry(this.total(), this.rowH(), this.viewportH()));
  protected readonly spacerH = computed(() => this.geometry().maxScroll);

  /** The row indexes in view (with overscan) and their positions, with the rows loaded so far. */
  private readonly visible = computed(() => {
    this.rows.version();
    const g = this.geometry();
    const top = this.virtualTop();
    const { first, last } = visibleWindow(g, top, OVERSCAN);
    const out: { index: number; y: number; row: RecordView | null }[] = [];
    for (let i = first; i < last; i++) out.push({ index: i, y: rowY(g, i, top), row: this.rows.rowAt(i) });
    return out;
  });

  private readonly rowIdPrefix = `events-row-${++gridSeq}-`;

  /**
   * The row keyboard focus is on (the selection's focus), named by the grid with aria-activedescendant so screen
   * readers announce it while focus stays on the grid. Null while that row is not rendered (scrolled away or loading).
   */
  protected readonly activeRowId = computed(() => {
    const focus = this.selection().focus;
    if (focus < 0) return null;
    const vm = this.rowVms().find((v) => v.index === focus);
    return vm?.kind === 'row' ? vm.id : null;
  });

  /** What each row in view shows (review #9: no per-row method calls in the template). */
  protected readonly rowVms = computed<GridRowVm[]>(() => {
    const sel = this.selection();
    const selectedId = this.sel.selected()?.id;
    const tz = this.prefs.timezone();
    const dateFormat = this.settings.dateFormat();
    const columns = this.prefs.extraColumns();
    const kinds = this.data.markerKindMap();
    const notes = this.notes.byEvent();
    return this.visible().map(({ index, y, row }): GridRowVm => {
      if (!row) return { kind: 'placeholder', index, y };
      const mark = this.colors.forRecord(row);
      const first = row.markers[0];
      const kind = first === undefined ? undefined : kinds.get(first);
      return {
        kind: 'row',
        index,
        y,
        row,
        id: this.rowIdPrefix + index,
        // In the grid's selection; while the grid has none (selected elsewhere, e.g. on the timeline), the inspector's record.
        selected: sel.anchor >= 0 ? contains(sel, index) : row.id === selectedId,
        highlight: this.highlight.colorFor(row),
        mark,
        stripe: mark ? stripeBackground(mark.swatch) : null,
        marker: first === undefined ? null : { kind: first, color: kind?.color ?? null, label: kind?.label ?? first },
        note: noteLabel(notes.get(row.id)),
        time: formatDateTime(row.ts, tz, true, dateFormat),
        untimed: row.tsState === 'untimed' || row.ts === null,
        levelVar: LEVEL_VARS[row.level],
        levelCode: LEVEL_CODES[row.level],
        level: row.level,
        quietLevel: row.level === 'Information' || row.level === 'Debug' || row.level === 'Verbose',
        ...splitInstance(row.group),
        cells: columns.map((column) => ({ column, text: this.cell(row, column) })),
      };
    });
  });

  constructor() {
    const destroyRef = inject(DestroyRef);
    destroyRef.onDestroy(() => {
      this.rows.dispose();
      // Nothing keeps the count current once the grid is gone: the status bar hides it instead of showing an old one.
      this.sel.setMatchTotal(null);
    });

    // New query, sort order or data: start from the top, except that a live refresh keeps the position.
    effect(() => {
      const query = this.queryStore.effectiveQuery();
      const descending = this.prefs.descending();
      this.data.dataVersion();
      untracked(() => {
        const key = JSON.stringify(query);
        const sameView = this.queryKey === key && descending === this.rows.descending;
        this.queryKey = key;
        if (sameView && this.live.isLiveRefresh()) {
          if (!this.feed.holdGrid()) this.liveRefresh('data');
        } else this.reset(query, descending);
      });
    });

    // "N new events" clicked, or following resumed: reload and bring the newest events into view.
    effect(() => {
      const nonce = this.feed.revealNonce();
      untracked(() => {
        if (nonce > 0 && this.rows.query) this.liveRefresh(this.follow.holding() ? 'reveal' : 'followOn');
      });
    });

    // Follow off, a pause or live tail off: the grid holds (counts) instead of following (ADR 0072, decision 3).
    effect(() => {
      const holding = this.follow.holding();
      untracked(() => {
        this.step({ type: holding ? 'hold' : 'released' });
      });
    });

    // Fill in pages the viewport needs; while the user scrolls, only once scrolling pauses. Other changes (data, a
    // jump) fetch at once.
    effect(() => {
      const missing = this.visible().some((v) => !v.row);
      if (!missing) return;
      untracked(() => this.rows.fill(() => this.missingRows()));
    });

    // effect-state: publishes the selection to the palette-command store (outside the grid)
    effect(() => {
      const anchor = this.sel.anchor();
      if (!anchor || anchor.nonce === handledAnchorNonce.get(this.sel)) return;
      handledAnchorNonce.set(this.sel, anchor.nonce);
      untracked(() => void this.jumpTo(anchor.id));
    });

    // Publish the selected records (and again as their pages load) for commands outside the grid.
    effect(() => {
      this.selection();
      this.rows.version();
      untracked(() => this.published.publish(this.selectedRecords()));
    });
    destroyRef.onDestroy(() => this.published.clear());

    document.addEventListener('pointerup', this.onPointerUp);
    document.addEventListener('pointercancel', this.onPointerUp);
    destroyRef.onDestroy(() => {
      document.removeEventListener('pointerup', this.onPointerUp);
      document.removeEventListener('pointercancel', this.onPointerUp);
    });

    afterNextRender(() => {
      const el = this.viewport()!.nativeElement;
      const measure = () => {
        const height = Math.max(0, el.clientHeight - HEADER_H);
        if (height === this.viewportH()) return;
        const first = !this.measured;
        this.measured = true;
        this.viewportH.set(height);
        if (!first) this.step({ type: 'resize' });
      };
      const ro = new ResizeObserver(measure);
      ro.observe(el);
      destroyRef.onDestroy(() => ro.disconnect());
      measure();
    });
  }

  private missingRows(): number[] {
    return this.visible()
      .filter((v) => !v.row)
      .map((v) => v.index);
  }

  private reset(query: Parameters<PagedRowsStore['reset']>[0], descending: boolean): void {
    this.pendingSelect = null;
    // A jump that gave up because the query kept changing reports on the view it ended on: that reset keeps it.
    if (this.jumpErrorView !== viewKey(query, descending)) this.jumpError.set(null);
    this.selection.set(EMPTY_SELECTION);
    this.setTop(0);
    this.step({ type: 'reset' }, descending);
    this.rows.reset(query, descending);
    void this.checkSelected(query, descending);
  }

  /** Asks the engine whether the event the inspector shows is still in the result, so the inspector can say when it is not. */
  private async checkSelected(query: Parameters<PagedRowsStore['reset']>[0], descending: boolean): Promise<void> {
    const id = untracked(() => this.sel.selected())?.id;
    if (id === undefined) {
      this.selectedCheck.cancel();
      return;
    }
    const answer = await this.selectedCheck.run(() => this.backend.queryPage({ query, offset: 0, limit: 1, descending, anchor: id }));
    // A failed check is unknown: the inspector keeps the event without a note rather than claiming it is hidden.
    if (answer.status === 'done') this.sel.setSelectedHidden(id, answer.value.anchorIndex === null);
  }

  /**
   * One step of the follow and hold state machine (ADR 0072): the pure `followStep` says what to do, this does it. Returns the
   * action so a caller can read it.
   */
  private step(event: FollowEvent, descending = this.rows.descending) {
    const { state, action } = followStep(this.followState, event, descending);
    this.followState = state;
    if (action.endHold) {
      this.pendingWrite = null;
      this.rows.endReveal();
    }
    if (action.writeNewest || action.keepTopRow) {
      const top = action.writeNewest ? (descending ? 0 : Number.POSITIVE_INFINITY) : this.virtualTop();
      this.setTop(top, this.total(), true);
      this.logger.debug('ui.grid', 'Resize re-applied', { PinnedWrite: action.writeNewest, Top: top });
    }
    return action;
  }

  /** Live tail changed the data, or the user asked for the newest events: refetch what is on screen (old rows stay until the new ones arrive). */
  private liveRefresh(event: 'data' | 'reveal' | 'followOn'): void {
    // A view that already is on the newest end (a result that fits the viewport) is pinned: the reducer decides, the position is its input.
    if (event === 'data') this.step({ type: 'positioned', atNewest: this.atNewest(this.virtualTop()), byUser: false });
    const { round, goNewest } = this.step({ type: event });
    if (round === 'none') return;
    if (goNewest && this.rows.descending) this.setTop(0);
    const g = this.geometry();
    const { first, last } = visibleWindow(g, this.virtualTop(), 0);
    this.jumpError.set(null);
    this.rows.liveRefresh(round, first, last);
  }

  /**
   * A page outside a live round reported a larger total (an event was pushed before the engine read it). The reducer says what that does to
   * the view: a pinned view follows the end (oldest first), a following view that is not pinned shifts by the extra rows (newest first), and a
   * held view that is not pinned keeps its rows.
   */
  private applyGrowth(newTotal: number, delta: number): void {
    const { growth } = this.step({ type: 'grew' });
    this.logger.debug('ui.grid', 'Total grew outside a round', { NewTotal: newTotal, Delta: delta, Growth: growth });
    if (growth === 'end') this.applyLive({ mode: 'end', newTotal, delta });
    else if (growth === 'shift') this.applyLive({ mode: 'shift', newTotal, delta });
  }

  /** After the first response of a live refresh: follow the end, or keep the same events in view. */
  private applyLive({ mode, newTotal, delta }: LiveArrival): void {
    this.logger.debug('ui.grid', 'Live arrival', { Mode: mode, NewTotal: newTotal, Delta: delta, Top: this.virtualTop() });
    if (mode === 'end') {
      this.go(Number.POSITIVE_INFINITY, newTotal, { afterRender: true });
      return;
    }
    if (delta <= 0) return;
    if (mode === 'shift') {
      this.go(this.virtualTop() + delta * this.rowH(), newTotal, { afterRender: true });
      this.selection.update((sel) => shift(sel, delta));
    }
  }

  protected retry(): void {
    this.jumpError.set(null);
    this.rows.retry(this.missingRows());
  }

  /**
   * Jumps to record `id` under the current query and sort order (ADR 0071). When the query changes while the engine
   * answers (a restored search box's debounce, smart search), the answer is for the old result: it asks again, at most
   * `JUMP_ATTEMPTS` times in all, then says so. A live refresh does not change the query and does not count. A newer
   * jump, the user's own selection and the grid's destruction make the jump stale (`LatestRequest`): a stale answer or
   * error is never acted on.
   */
  private async jumpTo(id: number): Promise<void> {
    for (let attempt = 1; ; attempt++) {
      const query = untracked(() => this.queryStore.effectiveQuery());
      const descending = untracked(() => this.prefs.descending());
      const asked = viewKey(query, descending);
      const answer = await this.jumps.run(() => this.backend.queryPage({ query, offset: 0, limit: 1, descending, anchor: id }));
      if (answer.status === 'stale') {
        this.logger.debug('ui.grid', 'Jump superseded', { Attempt: attempt });
        return;
      }
      if (asked !== this.viewKeyNow()) {
        if (attempt < JUMP_ATTEMPTS) {
          this.logger.info('ui.grid', 'Query changed during a jump; asking again', { Attempt: attempt, Failed: answer.status === 'failed' });
          continue;
        }
        this.logger.warn('ui.grid', 'Jump given up: the query kept changing', { Attempts: attempt });
        this.failJump($localize`:@@grid.jumpGaveUp:Could not go to the event: the filters kept changing. Try again.`);
        return;
      }
      if (answer.status === 'failed') {
        this.logger.warn('ui.grid', 'Jump failed', { Attempt: attempt, Error: describeError(answer.error) });
        this.failJump(describeError(answer.error));
        return;
      }
      const res = answer.value;
      this.logger.debug('ui.grid', 'Jump resolved', { Attempt: attempt, Found: res.anchorIndex !== null });
      if (res.anchorIndex === null || !res.rows.length) {
        this.sel.anchorResolved(null);
        return;
      }
      this.rows.setTotal(res.total);
      this.selection.set(single(res.anchorIndex));
      this.sel.anchorResolved(res.rows[0] ?? null);
      this.revealRow(res.anchorIndex, 'center', res.total);
      return;
    }
  }

  /** The query and order the grid shows now (or is about to: the reset effect may not have run yet). */
  private viewKeyNow(): string {
    return viewKey(
      untracked(() => this.queryStore.effectiveQuery()),
      untracked(() => this.prefs.descending()),
    );
  }

  private failJump(text: string): void {
    this.jumpError.set(text);
    this.jumpErrorView = this.viewKeyNow();
  }

  /** The user selected a row: a jump still on its way must not move the selection afterwards. */
  private selectByUser(row: RecordView): void {
    this.jumps.cancel();
    this.sel.select(row);
  }

  private revealRow(index: number, mode: 'nearest' | 'center', total = this.total()): void {
    const g = makeGeometry(total, this.rowH(), this.viewportH());
    this.go(topForRow(g, index, mode, this.virtualTop()), total, { byUser: true });
  }

  /** Whether the virtual offset `top` is on the newest end (the last row oldest first, the first row newest first). */
  private atNewest(top: number, g = this.geometry()): boolean {
    return this.rows.descending ? top < g.rowH : top >= g.maxTop - g.rowH;
  }

  /**
   * Moves the view to `top` of `total` rows and tells the follow state machine where it landed (`positioned`: `byUser` for a key, a jump
   * or a wheel in the scaled mode, not for a live write or a resize). `pinned` changes only through that event (ADR 0072, decision 3).
   */
  private go(top: number, total = this.total(), options: { afterRender?: boolean; byUser?: boolean } = {}): void {
    const t = this.setTop(top, total, options.afterRender ?? false);
    const g = makeGeometry(total, this.rowH(), this.viewportH());
    this.step({ type: 'positioned', atNewest: this.atNewest(t, g), byUser: options.byUser ?? false });
  }

  /**
   * The writer: the position is set at once and is the source of truth; the scroll write is the only place that writes `scrollTop`
   * (ADR 0072, decision 2). With `afterRender` it waits for the render (the size changes: a live arrival or a resize): a browser
   * clamps a scroll written before the spacer has grown and reports the clamped position, so writing early leaves the view short
   * or, in WebKit, at the top. At most one write waits; a newer one replaces it and a user take-over drops it (`step` on `endHold`), so a
   * deferred write never overwrites the user's position. Returns the clamped position. It does not touch the follow state.
   */
  private setTop(top: number, total = this.total(), afterRender = false): number {
    const g = makeGeometry(total, this.rowH(), this.viewportH());
    const t = clamp(top, 0, g.maxTop);
    this.virtualTop.set(t);
    const physical = Math.round(topToScroll(g, t));
    const write = (): void => {
      const el = this.viewport()?.nativeElement;
      if (!el) return;
      const read = writeScrollTop(el, physical);
      this.writtenScroll = physical;
      // A write in the middle of a gesture (the pointer is held) does not end it: the drag's next scroll is still the user's.
      if (!this.pointerDown) this.inputSinceWrite = false;
      this.writtenLayout = this.layoutKey();
      this.writtenMax = el.scrollHeight - el.clientHeight;
      this.logger.debug('ui.grid', 'Scroll written', { Physical: physical, Read: read, Max: el.scrollHeight - el.clientHeight });
    };
    // A write beyond what the rendered spacer can scroll to (the total grew and the render has not caught up) would be clamped by the
    // browser: it waits for the render too, whoever asks (a key to the end, a jump).
    const el = this.viewport()?.nativeElement;
    const staleSpacer = !!el && el.scrollHeight > 0 && physical > el.scrollHeight - el.clientHeight + 1;
    if (!afterRender && !staleSpacer) {
      this.pendingWrite = null;
      write();
      return t;
    }
    const slot = { physical };
    this.pendingWrite = slot;
    this.logger.debug('ui.grid', 'Scroll write scheduled', { Physical: physical, Total: total });
    afterNextRender(
      () => {
        if (this.pendingWrite !== slot) {
          this.logger.debug('ui.grid', 'Scroll write dropped', { Physical: physical });
          return;
        }
        this.pendingWrite = null;
        write();
      },
      { injector: this.injector },
    );
    return t;
  }

  /**
   * An input event that moves the view away from the newest end (a wheel or key away, a press on the scrollbar, a touch): the user took
   * the view over, so the round lets go, a pending write is dropped and the view is no longer pinned. An input toward the newest end, or
   * one that cannot move the view, takes nothing over (xterm does the same). Being back on the newest end pins again only through a
   * position event (a scroll the grid did not write, a key that lands there, the release of the scrollbar).
   */
  protected userTookOver(input = 'input'): void {
    this.logger.debug('ui.grid', 'User took over', { Input: input, Pinned: this.followState.pinned });
    this.step({ type: 'userTookOver' });
  }

  /** The direction `up` or `down` is away from the newest end: oldest first the newest end is at the bottom, newest first at the top. */
  private movesAway(direction: 'up' | 'down'): boolean {
    return this.rows.descending ? direction === 'down' : direction === 'up';
  }

  protected onPointerDown(e: PointerEvent): void {
    this.inputSinceWrite = true;
    // The scrollbar is part of the scroller itself; a press on a row is a selection, not a scroll.
    if (e.target !== this.viewport()?.nativeElement) return;
    this.pointerDown = true;
    this.userTookOver('scrollbar');
  }

  protected onTouchStart(e: TouchEvent): void {
    this.inputSinceWrite = true;
    if (e.target === this.viewport()?.nativeElement) this.userTookOver('touch');
  }

  /** The scrollbar was released: if the user let go on the newest end, the view is pinned there again. */
  private onPointerUp = (): void => {
    if (!this.pointerDown) return;
    this.pointerDown = false;
    this.step({ type: 'positioned', atNewest: this.atNewest(this.virtualTop()), byUser: true });
  };

  protected onScroll(): void {
    const el = this.viewport()?.nativeElement;
    if (!el) return;
    this.rows.noteScroll();
    const g = this.geometry();
    const read = el.scrollTop;
    const origin = classifyScroll({ read, pendingWrite: this.pendingWrite?.physical ?? null, lastWrite: this.writtenScroll, maxScroll: el.scrollHeight - el.clientHeight, recentInput: this.inputSinceWrite || this.pointerDown, layoutChanged: this.layoutKey(el) !== this.writtenLayout, layoutDelta: Math.abs(el.scrollHeight - el.clientHeight - this.writtenMax) });
    // Only a change of origin is logged (each log line is an IPC call in the desktop app, and a drag fires many scroll events).
    if (origin !== this.lastOrigin) {
      this.lastOrigin = origin;
      this.logger.debug('ui.grid', 'Scroll origin', { Origin: origin, Read: read, Written: this.writtenScroll, Pending: this.pendingWrite?.physical ?? null });
    }
    // Our own write (or its clamped echo): the position signal already says where the view is; reading it back would lose the row-exact position.
    if (origin === 'own') return;
    if (origin === 'spurious') {
      this.setTop(this.virtualTop(), this.total(), true); // the browser moved it: write our own position back after the render
      return;
    }
    this.writtenScroll = Number.NaN;
    const top = scrollToTop(g, read);
    this.virtualTop.set(top);
    // The user moved the view. While the scrollbar is held the view is never re-pinned (the release decides).
    this.step(this.pointerDown ? { type: 'userTookOver' } : { type: 'positioned', atNewest: this.atNewest(top, g), byUser: true });
  }

  /** With a scaled scrollbar the wheel moves by pixels of the true content, not of the compressed spacer. */
  protected onWheel(e: WheelEvent): void {
    this.inputSinceWrite = true;
    if (e.deltaY !== 0 && this.movesAway(e.deltaY < 0 ? 'up' : 'down')) this.userTookOver('wheel');
    const g = this.geometry();
    if (!g.scaled || e.ctrlKey || e.deltaY === 0) return;
    e.preventDefault();
    this.go(this.virtualTop() + wheelToPixels(e.deltaY, e.deltaMode, g), this.total(), { byUser: true });
  }

  /** Dragging a row carries the event to other views (drop it on Notes to write a note on it). */
  protected onRowDragStart(e: DragEvent, row: RecordView): void {
    if (!e.dataTransfer) return;
    this.selectByUser(row);
    writeEventDrag(e.dataTransfer, row);
  }

  /** Keeps shift/ctrl-click from also selecting text across rows. */
  protected onRowMouseDown(e: MouseEvent): void {
    // A draggable row cannot start a text selection in WebKit: the message cell is for selecting, the rest of the row for dragging.
    const row = e.currentTarget as HTMLElement | null;
    if (row && e.target instanceof Element) row.draggable = !e.target.closest('.msg');
    if (e.shiftKey || e.ctrlKey || e.metaKey) e.preventDefault();
  }

  /** Click: only this row; Ctrl/Cmd+click: add or remove it; Shift+click: the range from the anchor. */
  protected onRowClick(e: MouseEvent, index: number, row: RecordView): void {
    const sel = this.selection();
    if (e.shiftKey) this.selection.set(extendTo(sel, index));
    else if (e.ctrlKey || e.metaKey) this.selection.set(toggle(sel, index));
    else this.selection.set(single(index));
    this.selectByUser(row);
  }

  /** Right-click inside the selection acts on all selected rows; elsewhere it selects and acts on that row only. */
  protected openRowMenu(e: MouseEvent, index: number, row: RecordView): void {
    e.preventDefault();
    const sel = this.selection();
    this.selection.set(contains(sel, index) ? withFocus(sel, index) : single(index));
    this.selectByUser(row);
    this.actions.openFor(e, row, { records: this.selectedRecords() });
  }

  /** The loaded records of the selection in grid order (rows of pages not in memory are skipped), capped. */
  private selectedRecords(): RecordView[] {
    return indices(this.selection(), MAX_ACTION_RECORDS)
      .map((i) => this.rows.rowAt(i))
      .filter((r): r is RecordView => r !== null);
  }

  protected onKey(e: KeyboardEvent): void {
    this.inputSinceWrite = true;
    const total = this.total();
    if (!total) return;
    if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      const selected = this.sel.selected();
      if (selected) {
        e.preventDefault();
        const focus = this.selection().focus;
        const rowEl =
          this.viewport()?.nativeElement.querySelector<HTMLElement>(`.row[aria-rowindex="${focus + 2}"]`) ??
          this.viewport()?.nativeElement.querySelector<HTMLElement>('.row.sel') ??
          (e.currentTarget as HTMLElement);
        const records = contains(this.selection(), focus) ? this.selectedRecords() : [];
        this.actions.openAtElement(rowEl, selected, { records }, e.currentTarget as HTMLElement);
      }
      return;
    }
    const sel = this.selection();
    const next = navigate(this.geometry(), e.key, sel.focus);
    if (next === null) return;
    e.preventDefault();
    // A key that moves away from the newest end takes the view over; one toward it only moves (and pins when it lands there).
    if (this.rows.descending ? ['ArrowDown', 'PageDown', 'End'].includes(e.key) : ['ArrowUp', 'PageUp', 'Home'].includes(e.key)) this.userTookOver('key ' + e.key);
    // Shift+arrow (or Shift+Page/Home/End) extends the selection from its anchor.
    this.selection.set(e.shiftKey && sel.anchor >= 0 ? extendTo(sel, next) : single(next));
    this.revealRow(next, 'nearest');
    const row = this.rows.rowAt(next);
    if (row) {
      this.pendingSelect = null;
      this.selectByUser(row);
    } else {
      this.jumps.cancel();
      this.pendingSelect = next;
      this.rows.ensurePage(this.rows.pageOf(next));
    }
  }

  private selectPending(): void {
    const index = this.pendingSelect;
    if (index === null) return;
    const row = this.rows.rowAt(index);
    if (!row) return;
    this.pendingSelect = null;
    if (this.selection().focus === index) this.sel.select(row);
  }

  protected toggleColumn(f: string): void {
    if (this.prefs.extraColumns().includes(f)) this.prefs.removeColumn(f);
    else this.prefs.addColumn(f);
  }

  private cell(r: RecordView, c: string): string {
    switch (c) {
      case '@source':
        return r.sourceName;
      case '@line':
        return String(r.line);
      case '@template':
        return r.template ?? '';
      default:
        return r.fields[c] === undefined ? '' : this.formatters.format(c, r.fields[c], r);
    }
  }
}
