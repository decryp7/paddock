import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { AppTally, Issue, IssueKind } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { AsyncState } from '../../shared/async-state';
import { Chart } from '../../shared/chart';
import { formatClock, formatDate, formatRelative, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ISSUE_KIND_LABEL, ISSUE_SOURCE_LABEL, type Tone } from '../../shared/labels';
import { Notifier } from '../../shared/notifier';
import { ISSUE_WINDOWS, IssuesStore } from '../../state/issues.store';
import { Lookups } from '../lookup/lookups';
import {
  type IssueFilter,
  type IssueRow,
  NO_FILTER,
  STABILITY_LABEL,
  filterIssues,
  issueRows,
  readSources,
  scoreOption,
  stabilityTone,
  trendText,
  unavailableSources,
} from './issues-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from '../../shared/full-text-tip';
import { SearchField } from '../../shared/search-field';
import { MatMenuModule } from '@angular/material/menu';
import { RowMenu } from '../../shared/row-menu';

/**
 * What the operating system recorded going wrong: a stability score for today and the last days, the apps and kinds with
 * the most trouble, and every event in one table grouped by day with its evidence in a side panel. Dyno only reads the
 * system's own logs; the event text is shown as plain text.
 */
@Component({
  selector: 'dy-issues-view',
  imports: [RowMenu, MatMenuModule, MatIconButton, SearchField, FullTextTip, MatTooltip, SuSegmentedControl, MatButton, AsyncState, Chart, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown)': 'key($event)' },
  template: `
    <dy-async [loaded]="!!store.overview()" [error]="store.error()" what="issues" [rows]="5" (retry)="store.load()">
      @if (store.overview(); as o) {
        <section class="hero" aria-label="Stability">
          <div class="score">
            <div class="big num" [attr.data-tone]="tone()">{{ o.score === null ? '—' : o.score }}</div>
            <div class="what">
              <strong [attr.data-tone]="tone()">{{ o.rating ? ratingLabel(o.rating) : 'Not known' }}</strong>
              <span>{{ o.score === null ? 'No system log could be read' : 'Today, out of 100' }}</span>
            </div>
          </div>
          @if (o.daily.length) {
            <dy-chart class="spark" [options]="option()" [label]="'Stability score per day over the last ' + o.days + ' days, 0 to 100'" />
          }
          <p class="trend">{{ trend() }}</p>
        </section>

        @for (u of unavailable(); track u.label) {
          <p class="note" role="status"><dy-icon name="info" /> <span><strong>{{ u.label }}</strong> could not be read: {{ u.reason }}</span></p>
        }

        @if (o.apps.length || o.kinds.length) {
          <div class="boards">
            @if (o.apps.length) {
              <table class="table board" aria-label="Apps with the most issues">
                <thead><tr><th>App</th><th class="r">Issues</th><th class="r">Latest</th></tr></thead>
                <tbody>
                  @for (a of o.apps.slice(0, 5); track a.name) {
                    <tr class="pick" tabindex="0" [class.on]="filter().app === a.name" [attr.aria-label]="'Show only ' + a.name" (click)="toggleApp(a)" (keydown.enter)="toggleApp(a)">
                      <td><span class="ell" [dyFullText]="a.name">{{ a.name }}</span></td>
                      <td class="r">{{ a.count }}</td>
                      <td class="r muted">{{ ago(a.last) }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            }
            @if (o.kinds.length) {
              <table class="table board" aria-label="Issues by kind">
                <thead><tr><th>Kind</th><th class="r">Issues</th></tr></thead>
                <tbody>
                  @for (k of o.kinds; track k.kind) {
                    <tr class="pick" tabindex="0" [class.on]="filter().kind === k.kind" [attr.aria-label]="'Show only ' + kindLabel(k.kind)" (click)="toggleKind(k.kind)" (keydown.enter)="toggleKind(k.kind)">
                      <td>{{ kindLabel(k.kind) }}</td>
                      <td class="r">{{ k.count }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            }
          </div>
        }

        <div class="bar" role="group" aria-label="Filters">
          <dy-search-field label="Search issues" placeholder="Search apps and messages" [value]="filter().search" (valueChange)="setFilter({ search: $event })" />
          <pd-segmented-control label="Period" [options]="windowSegs" [value]="store.days() + ''" (valueChange)="store.setDays(+$event)" />
          @if (filter().app || filter().kind) {
            <button type="button" matButton="outlined" [attr.aria-label]="'Clear the filter ' + (filter().app ?? kindLabel(filter().kind!))" (click)="clear()">{{ filter().app ?? kindLabel(filter().kind!) }} <dy-icon name="close" /></button>
          }
          <span class="grow"></span>
          <span class="faint src">{{ sourcesText() }}</span>
        </div>

        <div class="layout" [class.with-detail]="!!selected()">
          <div class="tablewrap">
            @if (rows().length === 0) {
              <div class="none">
                <dy-icon [name]="o.total === 0 ? 'check' : 'filter'" />
                <strong>{{ o.total === 0 ? 'No issues in the last ' + o.days + ' days' : 'Nothing matches this filter' }}</strong>
                <span>{{ o.total === 0 ? 'The system logs Dyno can read hold no crashes, hangs or other problems for this period.' : 'Clear the filter to see every issue again.' }}</span>
              </div>
            } @else {
              <table class="table issues" aria-label="Issues">
                <thead><tr><th class="time">Time</th><th class="kind">Kind</th><th>App or event</th><th>Details</th><th class="acts"><span class="visually-hidden">Actions</span></th></tr></thead>
                <tbody>
                  @for (r of rows(); track r.key) {
                    @if (r.kind === 'day') {
                      <tr class="day">
                        <td colspan="5">
                          <button type="button" matIconButton [matTooltip]="(r.open ? 'Collapse ' : 'Expand ') + r.label" class="chev" [attr.aria-expanded]="r.open" [attr.aria-label]="(r.open ? 'Collapse ' : 'Expand ') + r.label" (click)="toggleDay(r.key)"><dy-icon [name]="r.open ? 'chevron-down' : 'chevron-right'" /></button>
                          <strong>{{ r.label }}</strong><span class="faint cnt">{{ plural(r.count, 'issue') }}</span>
                        </td>
                      </tr>
                    } @else {
                      <tr class="row" tabindex="0" [attr.data-id]="r.issue.id" [class.sel]="selectedId() === r.issue.id" (click)="select(r.issue)" (keydown.enter)="select(r.issue)" (keydown.shift.f10)="select(r.issue)" (contextmenu)="select(r.issue)" dyRowMenu [matContextMenuTriggerFor]="menu" [matContextMenuTriggerData]="{ issue: r.issue }">
                        <td class="time num muted">{{ clock(r.issue.at) }}</td>
                        <td class="kind"><span class="dot" [attr.data-tone]="dotTone(r.issue)" aria-hidden="true"></span>{{ kindLabel(r.issue.kind) }}</td>
                        <td><span class="ell" [dyFullText]="subject(r.issue)">{{ subject(r.issue) }}</span></td>
                        <td class="muted"><span class="ell" [dyFullText]="r.issue.evidence">{{ r.issue.evidence || r.issue.title }}</span></td>
                        <td class="acts"><button type="button" matIconButton [attr.aria-label]="'Copy details of ' + subject(r.issue)" matTooltipPosition="left" [matTooltip]="'Copy details of ' + subject(r.issue)" (click)="copy(r.issue, $event)"><dy-icon name="copy" /></button></td>
                      </tr>
                    }
                  }
                </tbody>
              </table>
            }
          </div>

          @if (selected(); as s) {
            <aside class="detail" tabindex="-1" aria-label="Issue details" (keydown.escape)="selectedId.set(null)">
              <div class="dhead">
                <strong>{{ s.title }}</strong>
                <button type="button" matIconButton matTooltip="Close details" aria-label="Close details" (click)="selectedId.set(null)"><dy-icon name="close" /></button>
              </div>
              <dl>
                <dt>When</dt><dd>{{ when(s) }}</dd>
                <dt>Kind</dt><dd>{{ kindLabel(s.kind) }}</dd>
                @if (s.app) { <dt>App</dt><dd class="sel">{{ s.app }}</dd> }
                <dt>Read from</dt><dd>{{ sourceLabel(s) }}</dd>
              </dl>
              @if (s.evidence) {
                <h3>What the system said</h3>
                <pre class="evidence sel">{{ s.evidence }}</pre>
              }
              <div class="dacts">
                <button type="button" matButton="outlined" (click)="copy(s)"><dy-icon name="copy" /> Copy</button>
                @if (s.app) { <button type="button" matButton="outlined" (click)="setFilter({ app: s.app })"><dy-icon name="filter" /> Only this app</button> }
              </div>
            </aside>
          }
        </div>
      }
    </dy-async>

    <mat-menu #menu="matMenu" aria-label="Issue actions">
      <ng-template matMenuContent let-issue="issue">
        <button mat-menu-item (click)="copy(issue)"><dy-icon name="copy" /> Copy details</button>
        <button mat-menu-item (click)="searchWeb(issue)"><dy-icon name="search" /> Search the web…</button>
        @if (issue.app) { <button mat-menu-item (click)="setFilter({ app: issue.app })"><dy-icon name="filter" /> Only this app</button> }
        <button mat-menu-item (click)="setFilter({ kind: issue.kind })"><dy-icon name="filter" /> Only this kind</button>
      </ng-template>
    </mat-menu>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); align-content: start; }
    .chev { width: 22px; height: 22px; padding: 0; flex: none; --icon-size: 14px; }
    dy-async { display: grid; gap: var(--sp-3); }
    .hero { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--sp-2) var(--sp-6); align-items: center; padding: var(--sp-4) var(--sp-5); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .score { display: flex; align-items: center; gap: var(--sp-4); min-width: 200px; }
    .big { font-size: 44px; font-weight: var(--fw-semibold); line-height: 1; letter-spacing: -0.02em; }
    .big[data-tone='warn'] { color: var(--warn); }
    .big[data-tone='bad'] { color: var(--bad); }
    .what { display: grid; gap: 2px; font-size: var(--fs-sm); color: var(--text-3); }
    .what strong { font-size: var(--fs-md); font-weight: var(--fw-semibold); color: var(--text-1); }
    .what strong[data-tone='warn'] { color: var(--warn); }
    .what strong[data-tone='bad'] { color: var(--bad); }
    .spark { --chart-h: 96px; }
    .trend { grid-column: 1 / -1; margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .boards { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--sp-3); align-items: start; }
    .board { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: hidden; }
    .board .ell { display: block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
    .pick { cursor: pointer; }
    .pick.on td { background: var(--accent-soft); }
    .pick:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
    .grow { flex: 1; }
    .src { font-size: var(--fs-xs); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 340px; }
    .tablewrap { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: auto; max-height: 560px; min-width: 0; }
    .issues { table-layout: fixed; }
    .issues th.time, .issues td.time { width: 64px; }
    .issues th.kind, .issues td.kind { width: 190px; }
    .issues th.acts, .issues td.acts { width: 40px; padding: 0 4px; }
    .issues td { overflow: hidden; text-overflow: ellipsis; }
    .issues td.kind { display: table-cell; }
    .issues td.kind .dot { margin-right: var(--sp-2); vertical-align: 1px; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    tr.day td { height: 32px; background: var(--surface-inset); border-bottom: 1px solid var(--border-subtle); }
    .cnt { margin-left: var(--sp-2); }
    tr.day strong { font-weight: var(--fw-semibold); }
    tr.row { cursor: default; }
    tr.row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    tr.row.sel td { background: var(--accent-soft); }
    tr.row.sel td:first-child { box-shadow: inset 2px 0 0 var(--accent); }
    .issues td.acts { opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); text-align: right; }
    tr.row:hover td.acts, tr.row:focus-within td.acts, tr.row.sel td.acts { opacity: 1; }
    .none { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-8) var(--sp-5); text-align: center; color: var(--text-3); }
    .none dy-icon { --icon-size: 32px; }
    .none strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .detail { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3) var(--sp-4); display: grid; gap: var(--sp-2); }
    .dhead { display: flex; justify-content: space-between; align-items: flex-start; gap: var(--sp-2); }
    .dhead strong { overflow-wrap: anywhere; }
    h3 { margin: var(--sp-2) 0 0; font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--text-2); }
    dl { margin: 0; display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; }
    .evidence { margin: 0; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--surface-inset); font: var(--fw-regular) 12px / 1.45 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 220px; overflow: auto; }
    .sel { user-select: text; cursor: text; }
    .dacts { display: flex; gap: var(--sp-2); flex-wrap: wrap; margin-top: var(--sp-1); }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } .boards { grid-template-columns: minmax(0, 1fr); } .hero { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class IssuesView {
  protected readonly store = inject(IssuesStore);
  private readonly theme = inject(ThemeService);
  private readonly notifier = inject(Notifier);
  private readonly lookups = inject(Lookups);
  protected readonly windows = ISSUE_WINDOWS;
  protected readonly windowSegs: SuSegment[] = ISSUE_WINDOWS.map((d) => ({ value: String(d), label: `${d} days` }));
  protected readonly filter = signal<IssueFilter>(NO_FILTER);
  protected readonly selectedId = signal<string | null>(null);
  private readonly collapsed = signal<ReadonlySet<string>>(new Set());
  protected readonly plural = plural;
  protected readonly clock = formatClock;
  protected readonly kindLabel = (k: IssueKind) => ISSUE_KIND_LABEL[k];
  protected readonly ratingLabel = (r: keyof typeof STABILITY_LABEL) => STABILITY_LABEL[r];
  protected readonly sourceLabel = (i: Issue) => ISSUE_SOURCE_LABEL[i.source];

  protected readonly tone = computed<Tone>(() => stabilityTone(this.store.overview()?.rating ?? null));
  protected readonly option = computed(() => scoreOption(this.store.overview()?.daily ?? [], this.theme.tokens()));
  protected readonly unavailable = computed(() => unavailableSources(this.store.overview()?.sources ?? []));
  private readonly now = computed(() => this.store.overview()?.to ?? 0);
  protected readonly trend = computed(() => {
    const o = this.store.overview();
    return o ? trendText(o.trend.current, o.trend.previous, o.days) : '';
  });
  protected readonly sourcesText = computed(() => {
    const o = this.store.overview();
    if (!o) return '';
    const read = readSources(o.sources);
    const when = o.scannedAt === null ? '' : `, ${formatRelative(o.scannedAt, o.to)}`;
    return o.sources.length === 0 ? 'No log to read on this system' : `${read} of ${plural(o.sources.length, 'log')} read${when}`;
  });
  protected readonly visible = computed(() => filterIssues(this.store.overview()?.issues ?? [], this.filter()));
  protected readonly rows = computed<IssueRow[]>(() => issueRows(this.visible(), this.now(), this.collapsed()));
  protected readonly selected = computed(() => this.store.overview()?.issues.find((i) => i.id === this.selectedId()) ?? null);

  protected setFilter(f: Partial<IssueFilter>): void {
    this.filter.update((cur) => ({ ...cur, ...f }));
  }

  protected clear(): void {
    this.filter.update((f) => ({ ...f, app: null, kind: null }));
  }

  protected toggleApp(a: AppTally): void {
    this.filter.update((f) => ({ ...f, app: f.app === a.name ? null : a.name }));
  }

  protected toggleKind(k: IssueKind): void {
    this.filter.update((f) => ({ ...f, kind: f.kind === k ? null : k }));
  }

  protected toggleDay(key: string): void {
    this.collapsed.update((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  protected select(i: Issue): void {
    this.selectedId.set(i.id);
  }

  /** Up and Down move between rows. */
  protected key(e: KeyboardEvent): void {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('tr.row')];
    const at = rows.indexOf(document.activeElement as HTMLElement);
    const next = rows[at + (e.key === 'ArrowDown' ? 1 : -1)];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  }

  protected subject(i: Issue): string {
    return i.app || i.title;
  }

  protected dotTone(i: Issue): string {
    return i.severity === 'critical' ? 'bad' : i.severity === 'warning' ? 'warn' : 'info';
  }

  protected ago(at: number): string {
    return formatRelative(at, this.now());
  }

  protected when(i: Issue): string {
    return `${formatDate(i.at, this.now())}, ${formatClock(i.at)}`;
  }

  /** Searches for the OS's title and evidence for the issue (the engine removes private details before the text is shown). */
  protected searchWeb(i: Issue): void {
    void this.lookups.searchWeb(`${i.title}. ${i.evidence}`, i.app || i.title);
  }

  protected async copy(i: Issue, e?: Event): Promise<void> {
    e?.stopPropagation();
    const text = [`${this.kindLabel(i.kind)}: ${i.title}`, this.when(i), i.evidence].filter(Boolean).join('\n');
    try {
      await navigator.clipboard.writeText(text);
      this.notifier.show('Copied');
    } catch {
      this.notifier.show('Copying is not allowed here.', 'bad');
    }
  }
}
