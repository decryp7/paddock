import { ScrollingModule } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatFormField, MatPrefix } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { RequestRecord } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { Toasts } from '../../core/toasts';
import { clock, int } from '../../shared/format';
import { AppAvatar } from '../../shared/ui/app-avatar';
import { Panel } from '../../shared/ui/panel';
import { ItemsMenu, MoreButton, RowMenu, type MenuItem } from '../../shared/ui/menu';
import { moveRowFocus } from '../../shared/ui/row-nav';
import { shortModel } from '../dashboard/dashboard';
import { API_LABEL, CHOSEN_LABEL, secondsCell, statusText, tokensCell } from './request-labels';
import { RequestDetail } from './request-detail';
import { REQUEST_LIMIT, RequestsStore, type StatusFilter } from './requests.store';

/** Row height of the virtual table (px); fixed so the viewport can compute what to render. */
export const ROW_HEIGHT = 40;

/** Milliseconds as a compact table cell (`—` when absent). */
export function msCell(ms: number | null): string {
  if (ms === null) return '—';
  return ms >= 10_000 ? `${(ms / 1000).toFixed(0)} s` : ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)}`;
}

/** A column of the log: always shown, or one the user adds from the column menu (the detail panel has them all). */
export interface Column {
  id: 'time' | 'app' | 'model' | 'endpoint' | 'api' | 'chosen' | 'queued' | 'ttft' | 'cold' | 'total' | 'tokens' | 'status';
  label: string;
  /** Header text with its unit. */
  header: string;
  width: string;
  right?: boolean;
  optional?: boolean;
}

export const COLUMNS: readonly Column[] = [
  { id: 'time', label: 'Time', header: 'Time', width: '68px' },
  { id: 'app', label: 'App', header: 'App', width: 'minmax(88px, 160px)' },
  { id: 'model', label: 'Model', header: 'Model', width: 'minmax(88px, 1fr)' },
  { id: 'endpoint', label: 'Endpoint', header: 'Endpoint', width: 'minmax(120px, 1.2fr)' },
  { id: 'api', label: 'API', header: 'API', width: '84px', optional: true },
  { id: 'chosen', label: 'Chosen by', header: 'Chosen by', width: '96px', optional: true },
  { id: 'queued', label: 'Queued', header: 'Queued ms', width: '84px', right: true, optional: true },
  { id: 'ttft', label: 'First token', header: 'TTFT ms', width: '72px', right: true },
  { id: 'cold', label: 'Cold start', header: 'Cold start s', width: '92px', right: true, optional: true },
  { id: 'total', label: 'Total', header: 'Total s', width: '68px', right: true },
  { id: 'tokens', label: 'Tokens', header: 'Tokens in / out', width: '110px', right: true },
  { id: 'status', label: 'Status', header: 'Status', width: '132px' },
];

/** What a narrow list (the details are open, or the window is small) drops: the panel shows these. */
export const NARROW_HIDDEN: ReadonlySet<Column['id']> = new Set(['endpoint', 'tokens', 'api', 'chosen', 'queued', 'cold']);

/** What a compact list (a 900 px window) also drops: the total time, which the detail panel shows. */
export const COMPACT_HIDDEN: ReadonlySet<Column['id']> = new Set([...NARROW_HIDDEN, 'total']);

/** The columns shown and the grid templates that lay them out as the list narrows (the action column is always last):
 * every column, narrow (≤ 900 px), compact (≤ 680 px) and tiny (≤ 560 px). */
export function layout(extra: ReadonlySet<Column['id']>): { cols: Column[]; template: string; narrow: string; compact: string; tiny: string } {
  const cols = COLUMNS.filter((c) => !c.optional || extra.has(c.id));
  const grid = (list: readonly Column[]) => [...list.map((c) => c.width), '32px'].join(' ');
  const tiny = cols.filter((c) => TINY_SHOWN.has(c.id)).map((c) => (c.id === 'app' ? 'minmax(100px, 1fr)' : c.width));
  return {
    cols,
    template: grid(cols),
    narrow: grid(cols.filter((c) => !NARROW_HIDDEN.has(c.id))),
    compact: grid(cols.filter((c) => !COMPACT_HIDDEN.has(c.id))),
    tiny: [...tiny, '32px'].join(' '),
  };
}

/** The few columns left when the list is squeezed beside the details: when, who, and the one that matters, an error. */
export const TINY_SHOWN: ReadonlySet<Column['id']> = new Set(['time', 'app', 'status']);

/** The request log: every recent request on one line; the facts that are the same on every row live in the detail panel. */
@Component({
  selector: 'pw-requests-page',
  imports: [ScrollingModule, MatIconButton, MatFormField, MatPrefix, MatInput, MatMenu, MatMenuItem, MatMenuTrigger, MatIconModule, MatTooltipModule, Panel, SuEmptyState, SuSegmentedControl, RequestDetail, AppAvatar, ItemsMenu, MoreButton, RowMenu, PdSelect],
  providers: [RequestsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Requests</h1>
          <p class="sub">What your apps asked for. Prompts and responses stay hidden unless you open them.</p>
        </div>
        <div class="actions">
          <button type="button" matIconButton aria-label="Refresh" matTooltip="Refresh" [disabled]="store.loading()" (click)="store.load()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" /></button>
        </div>
      </header>

      <div class="filters" role="search" aria-label="Filter requests">
        <mat-form-field class="search">
          <mat-icon matPrefix svgIcon="pw:search" aria-hidden="true" />
          <input matInput id="pw-req-search" type="search" placeholder="Search endpoint or model" aria-label="Search endpoint or model" autocomplete="off" [value]="store.filter().text" (input)="store.setFilter({ text: $any($event.target).value })" />
        </mat-form-field>
        <pd-select class="app-filter" inputId="pw-req-app" ariaLabel="App" [options]="appSelectOptions()" [value]="store.filter().app" (valueChange)="store.setFilter({ app: $event })" />
        <pd-segmented-control label="Status" [options]="statusSegs()" [value]="store.filter().status" (valueChange)="setStatus($event)" />
        <span class="shown faint num" aria-live="polite">{{ shownText() }}</span>
        <button type="button" matIconButton class="cols" aria-label="Columns" matTooltip="Columns" [matMenuTriggerFor]="colsMenu">
          <mat-icon svgIcon="pw:sliders" aria-hidden="true" />
        </button>
        <mat-menu #colsMenu="matMenu" aria-label="Columns" class="pw-cols-menu">
          @for (c of optionalColumns; track c.id) {
            <button type="button" mat-menu-item role="menuitemcheckbox" class="opt" [attr.aria-checked]="extra().has(c.id)" (click)="$event.stopPropagation(); toggleColumn(c.id)">
              <mat-icon svgIcon="pw:check" aria-hidden="true" [class.off]="!extra().has(c.id)" />{{ c.label }}
            </button>
          }
        </mat-menu>
      </div>

      <div class="with-panel fill body">
        <pw-panel class="table-panel" [flush]="true" [loading]="store.loading()" [ready]="store.loaded()" [error]="store.error()" [skeletonLines]="10" (retry)="store.load()">
          <div class="tbl" role="table" aria-label="Requests" [attr.aria-rowcount]="store.visible().length + 1" [style.--cols]="grid().template" [style.--cols-narrow]="grid().narrow" [style.--cols-compact]="grid().compact" [style.--cols-tiny]="grid().tiny">
            <div class="sheet">
              <div class="row head" role="row">
                @for (c of grid().cols; track c.id) {
                  <span role="columnheader" [class]="'c-' + c.id" [class.r]="c.right" [class.sticky]="c.id === 'time'">{{ c.header }}</span>
                }
                <span role="columnheader"><span class="sr-only">Actions</span></span>
              </div>
              @if (store.visible().length === 0) {
                <pd-empty-state [heading]="store.records().length ? 'No request matches' : 'No requests yet'" [description]="store.records().length ? 'Change the filter to see more.' : 'Requests from your apps appear here.'"><mat-icon slot="icon" svgIcon="pw:requests" aria-hidden="true" /></pd-empty-state>
              } @else {
                <cdk-virtual-scroll-viewport class="vp fit" [itemSize]="rowHeight" minBufferPx="400" maxBufferPx="800" role="rowgroup">
                  <div
                    *cdkVirtualFor="let r of store.visible(); trackBy: trackId"
                    class="row"
                    role="row"
                    tabindex="0"
                    [class.sel]="r.id === store.selectedId()"
                    [class.failed]="r.status >= 400"
                    [attr.aria-label]="label(r)"
                    [attr.aria-selected]="r.id === store.selectedId()"
                    (click)="onRowClick(r, $event)"
                    (keydown)="onRowKey(r, $event)"
                    pwRowMenu
                    [matContextMenuTriggerFor]="rowMenu.menu()"
                    [matContextMenuTriggerData]="{ items: menuItems(r) }"
                  >
                    @for (c of grid().cols; track c.id) {
                      @switch (c.id) {
                        @case ('time') {
                          <span role="cell" class="c-time num t sticky">{{ time(r.at) }}</span>
                        }
                        @case ('app') {
                          <span role="cell" class="c-app app"><pw-app-avatar [key]="r.appId" [name]="appName(r.appId)" /><span class="nm">{{ appName(r.appId) }}</span></span>
                        }
                        @case ('model') {
                          <span role="cell" class="c-model model" [matTooltip]="modelTip(r)" matTooltipShowDelay="600">{{ r.modelId ? model(r.modelId) : '—' }}@if (r.alias) {<span class="faint via"> via {{ r.alias }}</span>}</span>
                        }
                        @case ('endpoint') {
                          <span role="cell" class="c-endpoint mono ep" [matTooltip]="r.endpoint" matTooltipShowDelay="600">{{ r.endpoint }}</span>
                        }
                        @case ('api') {
                          <span role="cell" class="c-api faint">{{ api[r.api] }}</span>
                        }
                        @case ('chosen') {
                          <span role="cell" class="c-chosen faint">{{ chosen[r.chosenBy] }}</span>
                        }
                        @case ('queued') {
                          <span role="cell" class="c-queued num r faint">{{ ms(r.queuedMs) }}</span>
                        }
                        @case ('ttft') {
                          <span role="cell" class="c-ttft num r">{{ ms(r.ttftMs) }}</span>
                        }
                        @case ('cold') {
                          <span role="cell" class="c-cold num r">{{ secs(r.coldStartMs) }}</span>
                        }
                        @case ('total') {
                          <span role="cell" class="c-total num r">{{ secs(r.totalMs) }}</span>
                        }
                        @case ('tokens') {
                          <span role="cell" class="c-tokens num r" [matTooltip]="intText(r.promptTokens) + ' in · ' + intText(r.completionTokens) + ' out'">{{ tokens(r.promptTokens, r.completionTokens) }}</span>
                        }
                        @case ('status') {
                          <span role="cell" class="c-status">
                            @if (r.status >= 400) {
                              <span class="status" [attr.data-tone]="r.status >= 500 ? 'bad' : 'warn'" [matTooltip]="r.error ?? ''">{{ statusLabel(r.status) }}</span>
                            } @else if (r.cache === 'hit') {
                              <span class="cached" matTooltip="Answered from the cache" matTooltipShowDelay="400"><mat-icon svgIcon="pw:bolt" aria-hidden="true" />cached</span>
                            }
                          </span>
                        }
                      }
                    }
                    <span role="cell" class="act"><pw-more-button [menu]="rowMenu.menu()" [items]="menuItems(r)" [label]="'More actions for the request to ' + r.endpoint" /></span>
                  </div>
                </cdk-virtual-scroll-viewport>
              }
            </div>
          </div>
        </pw-panel>

        @if (store.selected(); as r) {
          <pw-request-detail [r]="r" [appName]="appName(r.appId)" (closed)="store.select(null)" />
        }
      </div>
    </div>
    <pw-items-menu #rowMenu label="Request actions" />
  `,
  styles: `
    :host {
      display: block;
      height: 100%;
    }
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
      height: 100%;
      padding-bottom: var(--pd-space-6);
    }
    .page-head {
      margin-bottom: var(--pd-space-2);
    }
    .filters {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      min-height: 44px;
    }
    /* One toolbar row at every supported width: the search gives way first, then the count. */
    .search {
      flex: 0 1 280px;
      min-width: 160px;
    }
    .app-filter {
      flex: 0 1 160px;
      min-width: 120px;
    }
    .shown {
      margin-left: auto;
      font-size: var(--pd-text-label);
      white-space: nowrap;
    }
    /* A toolbar too narrow for everything drops the count first (the list shows the rows anyway). */
    .filters {
      container: req-filters / inline-size;
    }
    @container req-filters (max-width: 700px) {
      .shown {
        display: none;
      }
      .filters > .cols {
        margin-left: auto;
      }
    }
    .filters > .cols {
      flex: none;
    }
    /* The Columns menu: a check marks each column that is on. */
    .opt mat-icon {
      color: var(--pd-accent);
    }
    .opt mat-icon.off {
      visibility: hidden;
    }
    .body {
      flex: 1;
      min-height: 360px;
      align-items: stretch;
    }
    .table-panel {
      min-height: 0;
      overflow: hidden;
    }
    .tbl {
      container-type: inline-size;
      flex: 1;
      min-height: 0;
      overflow-x: auto;
      display: flex;
      flex-direction: column;
    }
    @container (max-width: 900px) {
      .row {
        grid-template-columns: var(--cols-narrow);
      }
      .c-endpoint,
      .c-tokens,
      .c-api,
      .c-chosen,
      .c-queued,
      .c-cold {
        display: none;
      }
    }
    @container (max-width: 680px) {
      .row {
        grid-template-columns: var(--cols-compact);
      }
      .c-total {
        display: none;
      }
    }
    @container (max-width: 560px) {
      .row {
        grid-template-columns: var(--cols-tiny);
      }
      .c-model,
      .c-ttft,
      .c-total {
        display: none;
      }
    }
    .sheet {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    .row {
      display: grid;
      grid-template-columns: var(--cols);
      align-items: center;
      gap: var(--pd-space-2);
      height: 40px;
      padding: 0 var(--pd-space-3) 0 var(--pd-space-4);
      font-size: var(--pd-text-label);
      border-bottom: 1px solid var(--pd-hairline);
      white-space: nowrap;
      cursor: default;
      outline: none;
      > span {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }
      &:hover {
        background: var(--pd-hover);
      }
      &:focus-visible {
        box-shadow: inset 0 0 0 2px var(--pd-accent-ring);
      }
      &.sel {
        background: var(--pd-accent-tint);
        box-shadow: inset 2px 0 0 var(--pd-accent);
      }
    }
    .row.head {
      height: 34px;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      color: var(--pd-text-subtle);
      border-bottom: 1px solid var(--pd-hairline);
      &:hover {
        background: none;
      }
    }
    /* The Time column stays when the window is too narrow for the table and it scrolls sideways. */
    .sticky {
      position: sticky;
      left: 0;
      z-index: 1;
      background: inherit;
    }
    .row:not(.head) .sticky {
      background: var(--pd-surface-1);
    }
    .row.sel .sticky {
      background: color-mix(in srgb, var(--pd-surface-1) 90%, var(--pd-accent));
    }
    .row:hover .sticky {
      background: color-mix(in srgb, var(--pd-surface-1) 95%, var(--pd-text));
    }
    .head .sticky {
      background: var(--pd-surface-1);
    }
    .r {
      text-align: right;
    }
    .cached {
      display: inline-flex;
      align-items: center;
      gap: 3px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-subtle);
      mat-icon {
        width: 12px;
        height: 12px;
      }
    }
    .c-status {
      padding-left: var(--pd-space-3);
    }
    .t {
      color: var(--pd-text-muted);
    }
    .app {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-2);
      .nm {
        overflow: hidden;
        text-overflow: ellipsis;
        font-weight: var(--pd-weight-medium);
      }
    }
    .ep {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .vp {
      flex: 1;
      min-height: 0;
    }
  `,
})
export class RequestsPage implements OnInit {
  protected readonly store = inject(RequestsStore);
  private readonly live = inject(LiveStore);
  private readonly toasts = inject(Toasts);
  protected readonly limit = REQUEST_LIMIT;
  protected readonly rowHeight = ROW_HEIGHT;
  protected readonly api = API_LABEL;
  protected readonly chosen = CHOSEN_LABEL;
  protected readonly ms = msCell;
  protected readonly secs = secondsCell;
  protected readonly tokens = tokensCell;
  protected readonly statusLabel = statusText;
  protected readonly intText = int;
  protected readonly model = shortModel;
  protected readonly optionalColumns = COLUMNS.filter((c) => c.optional);
  protected readonly statuses: { value: StatusFilter; label: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'ok', label: 'Succeeded' },
    { value: 'errors', label: 'Errors' },
  ];

  /** The optional columns the user turned on (none by default: they repeat on nearly every row). */
  protected readonly extra = signal<ReadonlySet<Column['id']>>(new Set());
  protected readonly grid = computed(() => layout(this.extra()));
  protected readonly shownText = computed(() => {
    const shown = this.store.visible().length;
    const all = this.store.records().length;
    return shown === all ? `${int(all)} requests` : `${int(shown)} of ${int(all)} requests`;
  });

  private readonly names = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));
  /** Apps seen in the records, named when Pitwall knows them. */
  protected readonly appOptions = computed(() => {
    const ids = [...new Set(this.store.records().map((r) => r.appId))];
    return ids.map((id) => ({ id, name: this.appName(id) })).sort((a, b) => a.name.localeCompare(b.name));
  });

  protected readonly appSelectOptions = computed<PdSelectOption[]>(() => [
    { value: 'all', label: 'All apps' },
    ...this.appOptions().map((a) => ({ value: a.id, label: a.name })),
  ]);

  ngOnInit(): void {
    void this.store.load();
  }

  /** The status filter as segments; Errors carries its count when there are any. */
  protected readonly statusSegs = computed<SuSegment[]>(() =>
    this.statuses.map((s) => ({ value: s.value, label: s.label, ...(s.value === 'errors' && this.store.errorCount() ? { count: this.store.errorCount() } : {}) })),
  );

  protected setStatus(value: string): void {
    const s = this.statuses.find((x) => x.value === value);
    if (s) this.store.setFilter({ status: s.value });
  }

  protected toggleColumn(id: Column['id']): void {
    this.extra.update((set) => {
      const next = new Set(set);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  protected appName(id: string): string {
    return this.names().get(id) ?? id;
  }

  protected time(at: number): string {
    return clock(at, true);
  }

  protected trackId(_: number, r: RequestRecord): string {
    return r.id;
  }

  protected modelTip(r: RequestRecord): string {
    return r.alias ? `${r.alias} resolved to ${r.modelId ?? 'no model'}` : (r.modelId ?? '');
  }

  protected label(r: RequestRecord): string {
    return `${clock(r.at, true)}, ${this.appName(r.appId)}, ${r.endpoint}, status ${r.status}${r.cache === 'hit' ? ', answered from the cache' : ''}`;
  }

  /** One list for the row's "..." button, its right-click menu and Shift+F10. */
  protected menuItems(r: RequestRecord): MenuItem[] {
    return [
      { label: 'Details', run: () => this.store.select(r.id) },
      { label: 'Copy request id', run: () => this.copy(r.id, 'Request id') },
      { label: 'Copy endpoint', run: () => this.copy(r.endpoint, 'Endpoint') },
      { label: `Only requests from ${this.appName(r.appId)}`, run: () => this.store.setFilter({ app: r.appId }), divider: true },
    ];
  }

  private copy(text: string, what: string): void {
    const clip = (globalThis as { navigator?: Navigator }).navigator?.clipboard;
    if (!clip) {
      this.toasts.show(`Copy ${what.toLowerCase()} failed: select it in the details instead`, 'bad');
      return;
    }
    clip.writeText(text).then(
      () => this.toasts.ok(`${what} copied`),
      () => this.toasts.show(`Copy ${what.toLowerCase()} failed: select it in the details instead`, 'bad'),
    );
  }

  protected onRowClick(r: RequestRecord, e: Event): void {
    if (!(e.target as HTMLElement).closest('button')) this.store.select(r.id);
  }

  protected onRowKey(r: RequestRecord, e: KeyboardEvent): void {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      this.store.select(r.id);
    } else {
      moveRowFocus(e);
    }
  }

}
