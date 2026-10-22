import { ScrollingModule } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { RequestRecord } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { clock, compact, int } from '../../shared/format';
import { statusTone } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { shortModel } from '../dashboard/dashboard';
import { API_LABEL, CHOSEN_LABEL } from './request-labels';
import { RequestDetail } from './request-detail';
import { REQUEST_LIMIT, RequestsStore, type StatusFilter } from './requests.store';

/** Row height of the virtual table (px); fixed so the viewport can compute what to render. */
export const ROW_HEIGHT = 38;

/** Milliseconds as a compact table cell (`—` when absent). */
export function msCell(ms: number | null): string {
  if (ms === null) return '—';
  return ms >= 10_000 ? `${(ms / 1000).toFixed(0)} s` : ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)}`;
}

/** The request inspector: every recent request with timing, routing and tokens; content only on demand. */
@Component({
  selector: 'pw-requests-page',
  imports: [ScrollingModule, MatIconModule, MatTooltipModule, Panel, EmptyState, RequestDetail],
  providers: [RequestsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Requests</h1>
          <p class="sub">The last {{ limit }} requests on this machine. Prompts and responses stay hidden unless you open them.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn" [disabled]="store.loading()" (click)="store.load()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" />Refresh</button>
        </div>
      </header>

      <div class="filters" role="search" aria-label="Filter requests">
        <div class="search">
          <mat-icon svgIcon="pw:search" aria-hidden="true" />
          <label class="sr-only" for="pw-req-search">Search endpoint or model</label>
          <input id="pw-req-search" class="input" type="search" placeholder="Search endpoint or model" autocomplete="off" [value]="store.filter().text" (input)="store.setFilter({ text: $any($event.target).value })" />
        </div>
        <label class="sr-only" for="pw-req-app">App</label>
        <select id="pw-req-app" class="select" [value]="store.filter().app" (change)="store.setFilter({ app: $any($event.target).value })">
          <option value="all">All apps</option>
          @for (a of appOptions(); track a.id) {
            <option [value]="a.id">{{ a.name }}</option>
          }
        </select>
        <div class="seg" role="radiogroup" aria-label="Status">
          @for (s of statuses; track s.value) {
            <button type="button" role="radio" [attr.aria-checked]="store.filter().status === s.value" (click)="store.setFilter({ status: s.value })">
              {{ s.label }}
              @if (s.value === 'errors' && store.errorCount()) {
                <span class="count num">{{ store.errorCount() }}</span>
              }
            </button>
          }
        </div>
        <span class="shown faint num" aria-live="polite">{{ store.visible().length }} of {{ store.records().length }}</span>
      </div>

      <pw-panel class="table-panel" [flush]="true" [loading]="store.loading()" [ready]="store.loaded()" [error]="store.error()" [skeletonLines]="10" (retry)="store.load()">
        <div class="tbl" role="table" aria-label="Requests" [attr.aria-rowcount]="store.visible().length + 1">
          <div class="row head" role="row">
            <span role="columnheader">Time</span>
            <span role="columnheader">App</span>
            <span role="columnheader">API</span>
            <span role="columnheader">Endpoint</span>
            <span role="columnheader">Model</span>
            <span role="columnheader">Chosen by</span>
            <span role="columnheader">Status</span>
            <span role="columnheader" class="r">Queued</span>
            <span role="columnheader" class="r">TTFT</span>
            <span role="columnheader" class="r">Total</span>
            <span role="columnheader" class="r">Cold start</span>
            <span role="columnheader" class="r">Tokens</span>
          </div>
          @if (store.visible().length === 0) {
            <pw-empty-state icon="requests" [heading]="store.records().length ? 'No request matches' : 'No requests yet'" [text]="store.records().length ? 'Change the filter to see more.' : 'Requests from your apps appear here.'" />
          } @else {
            <cdk-virtual-scroll-viewport class="vp" [itemSize]="rowHeight" minBufferPx="400" maxBufferPx="800" role="rowgroup">
              <div
                *cdkVirtualFor="let r of store.visible(); trackBy: trackId"
                class="row"
                role="row"
                tabindex="0"
                [class.sel]="r.id === store.selectedId()"
                [class.failed]="r.status >= 400"
                [attr.aria-label]="label(r)"
                (click)="store.select(r.id)"
                (keydown.enter)="store.select(r.id)"
                (keydown.space)="$event.preventDefault(); store.select(r.id)"
              >
                <span role="cell" class="num t">{{ time(r.at) }}</span>
                <span role="cell" class="app">{{ appName(r.appId) }}</span>
                <span role="cell" class="faint">{{ api[r.api] }}</span>
                <span role="cell" class="mono ep">{{ r.endpoint }}</span>
                <span role="cell" class="model">{{ r.modelId ? model(r.modelId) : '—' }}</span>
                <span role="cell" class="faint">{{ chosen[r.chosenBy] }}</span>
                <span role="cell"><span class="pill" [attr.data-tone]="tone(r.status)">{{ r.status }}</span></span>
                <span role="cell" class="num r faint">{{ ms(r.queuedMs) }}</span>
                <span role="cell" class="num r">{{ ms(r.ttftMs) }}</span>
                <span role="cell" class="num r">{{ ms(r.totalMs) }}</span>
                <span role="cell" class="num r" [class.cold]="r.coldStartMs !== null">{{ ms(r.coldStartMs) }}</span>
                <span role="cell" class="num r" [matTooltip]="intText(r.promptTokens) + ' in · ' + intText(r.completionTokens) + ' out'">{{ count(r.promptTokens + r.completionTokens) }}</span>
              </div>
            </cdk-virtual-scroll-viewport>
          }
        </div>
      </pw-panel>
    </div>

    @if (store.selected(); as r) {
      <pw-request-detail [r]="r" [appName]="appName(r.appId)" (closed)="store.select(null)" />
    }
  `,
  styles: `
    :host {
      display: block;
      height: 100%;
    }
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
      height: 100%;
      padding-bottom: var(--sp-6);
    }
    .page-head {
      margin-bottom: var(--sp-2);
    }
    .filters {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
    }
    .search {
      position: relative;
      width: 300px;
      mat-icon {
        position: absolute;
        left: 9px;
        top: 50%;
        width: 14px;
        height: 14px;
        transform: translateY(-50%);
        color: var(--text-3);
        pointer-events: none;
      }
      .input {
        width: 100%;
        padding-left: 30px;
      }
    }
    .select {
      min-width: 160px;
    }
    .seg {
      display: inline-flex;
      padding: 2px;
      background: var(--bg-active);
      border-radius: var(--r-md);
      button {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 26px;
        padding: 0 var(--sp-3);
        font: inherit;
        font-size: var(--fs-sm);
        font-weight: var(--fw-medium);
        color: var(--text-2);
        background: none;
        border: none;
        border-radius: var(--r-sm);
        cursor: pointer;
        &[aria-checked='true'] {
          color: var(--text);
          background: var(--bg-card);
          box-shadow: var(--shadow-1);
        }
      }
      .count {
        padding: 0 5px;
        font-size: var(--fs-2xs);
        color: var(--bad);
        background: var(--bad-soft);
        border-radius: var(--r-full);
      }
    }
    .shown {
      margin-left: auto;
      font-size: var(--fs-sm);
    }
    .table-panel {
      flex: 1;
      min-height: 360px;
      overflow: hidden;
    }
    .tbl {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
    }
    .row {
      display: grid;
      grid-template-columns: 76px 128px 84px minmax(150px, 1fr) minmax(120px, 170px) 92px 62px 64px 64px 68px 74px 68px;
      align-items: center;
      gap: var(--sp-2);
      height: 38px;
      padding: 0 var(--sp-4);
      font-size: var(--fs-sm);
      border-bottom: 1px solid var(--border-soft);
      white-space: nowrap;
      cursor: pointer;
      outline: none;
      > span {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }
      &:hover {
        background: var(--bg-hover);
      }
      &:focus-visible {
        box-shadow: inset 0 0 0 2px var(--accent-ring);
      }
      &.sel {
        background: var(--accent-soft);
      }
      &.failed .ep {
        color: var(--bad);
      }
    }
    .row.head {
      height: 34px;
      font-size: var(--fs-xs);
      font-weight: var(--fw-medium);
      color: var(--text-3);
      border-bottom: 1px solid var(--border);
      cursor: default;
      &:hover {
        background: none;
      }
    }
    .r {
      text-align: right;
    }
    .t {
      color: var(--text-2);
    }
    .app {
      font-weight: var(--fw-medium);
    }
    .ep {
      font-size: var(--fs-xs);
    }
    .cold {
      color: var(--warn);
      font-weight: var(--fw-medium);
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
  protected readonly limit = REQUEST_LIMIT;
  protected readonly rowHeight = ROW_HEIGHT;
  protected readonly api = API_LABEL;
  protected readonly chosen = CHOSEN_LABEL;
  protected readonly tone = statusTone;
  protected readonly ms = msCell;
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly model = shortModel;
  protected readonly statuses: { value: StatusFilter; label: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'ok', label: 'Succeeded' },
    { value: 'errors', label: 'Errors' },
  ];

  private readonly names = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));
  /** Apps seen in the records, named when Pitwall knows them. */
  protected readonly appOptions = computed(() => {
    const ids = [...new Set(this.store.records().map((r) => r.appId))];
    return ids.map((id) => ({ id, name: this.appName(id) })).sort((a, b) => a.name.localeCompare(b.name));
  });

  ngOnInit(): void {
    void this.store.load();
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

  protected label(r: RequestRecord): string {
    return `${clock(r.at, true)}, ${this.appName(r.appId)}, ${r.endpoint}, status ${r.status}`;
  }
}
