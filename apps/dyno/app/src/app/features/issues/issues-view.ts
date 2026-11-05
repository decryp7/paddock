import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { AppTally, Issue, IssueKind, KindTally } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { AsyncState } from '../../shared/async-state';
import { Chart } from '../../shared/chart';
import { formatClock, formatDate, formatRelative, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ISSUE_KIND_LABEL, ISSUE_SOURCE_LABEL, type Tone } from '../../shared/labels';
import { Notifier } from '../../shared/notifier';
import { ISSUE_WINDOWS, IssuesStore } from '../../state/issues.store';
import { Lookups } from '../lookup/lookups';
import { type IssueFilter, NO_FILTER, STABILITY_LABEL, filterIssues, issueRows, readSources, scoreOption, stabilityTone, trendText, unavailableSources } from './issues-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { SearchField } from '../../shared/search-field';
import { MatMenuModule } from '@angular/material/menu';

interface IssueLine extends Issue {
  readonly day: string;
}

/**
 * What the operating system recorded going wrong: a stability score for today and the last days, the apps and kinds with
 * the most trouble, and every event in one table grouped by day with its evidence in a side panel. Dyno only reads the
 * system's own logs; the event text is shown as plain text.
 */
@Component({
  selector: 'dy-issues-view',
  imports: [AsyncState, Chart, Icon, PdTable, PdCell, MatMenuModule, MatIconButton, SearchField, MatTooltip, SuSegmentedControl, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
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
              <pd-table height="window" [toolbar]="false" label="Apps with the most issues" [rows]="appBoard()" [columns]="appColumns" [rowKey]="appKey" [filterable]="false" [selectedKey]="filter().app" (rowActivate)="toggleApp($event)" />
            }
            @if (o.kinds.length) {
              <pd-table height="window" [toolbar]="false" label="Issues by kind" [rows]="o.kinds" [columns]="kindColumns" [rowKey]="kindKey" [filterable]="false" [selectedKey]="filter().kind" (rowActivate)="toggleKind($event.kind)" />
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
            @if (lines().length === 0) {
              <div class="none">
                <dy-icon [name]="o.total === 0 ? 'check' : 'filter'" />
                <strong>{{ o.total === 0 ? 'No issues in the last ' + o.days + ' days' : 'Nothing matches this filter' }}</strong>
                <span>{{ o.total === 0 ? 'The system logs Dyno can read hold no crashes, hangs or other problems for this period.' : 'Clear the filter to see every issue again.' }}</span>
              </div>
            } @else {
              <pd-table [toolbar]="false" label="Issues" [rows]="lines()" [columns]="columns" [rowKey]="lineKey" [filterable]="false" height="window" groupBy="day" [rowActions]="actions" [selectedKey]="selectedId()" (rowActivate)="select($event)" (action)="run($event)" (keydown.escape)="selectedId.set(null)">
                <ng-template pdCell="kind" let-i><span class="dot" [attr.data-tone]="dotTone(i)" aria-hidden="true"></span>{{ kindLabel(i.kind) }}</ng-template>
              </pd-table>
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
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
    .grow { flex: 1; }
    .src { font-size: var(--fs-xs); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 340px; }
    .tablewrap { min-width: 0; }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); flex: none; background: var(--text-3); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='info'] { background: var(--info); }
    .ib { display: inline-grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover { background: var(--surface-active); color: var(--text-1); }
    .ib dy-icon { --icon-size: 15px; }
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
  /** The issues, each with the day it belongs to (the model builds the day headings; the table groups by them and folds them). */
  protected readonly lines = computed<IssueLine[]>(() => {
    const out: IssueLine[] = [];
    let day = '';
    for (const r of issueRows(this.visible(), this.now(), new Set())) {
      if (r.kind === 'day') day = r.label;
      else out.push({ ...r.issue, day });
    }
    return out;
  });
  protected readonly lineKey = (i: IssueLine): string => i.id;
  protected readonly columns: PdColumn<IssueLine>[] = [
    { key: 'day', header: 'Day', value: (i) => i.day, hidden: true, sortable: false },
    { key: 'time', header: 'Time', value: (i) => i.at, text: (i) => formatClock(i.at), width: '64px', sortable: false },
    { key: 'kind', header: 'Kind', value: (i) => ISSUE_KIND_LABEL[i.kind], width: '190px', sortable: false },
    { key: 'subject', header: 'App or event', value: (i) => this.subject(i), required: true, width: 'minmax(0, 1fr)', sortable: false },
    { key: 'details', header: 'Details', value: (i) => i.evidence || i.title, width: 'minmax(0, 1.4fr)', hideBelow: 640, sortable: false },
  ];
  protected readonly actions = (i: IssueLine): PdRowAction[] => [
    { id: 'copy', label: 'Copy details', icon: 'copy', inline: true },
    { id: 'web', label: 'Search the web…', icon: 'search' },
    ...(i.app ? [{ id: 'app', label: 'Only this app', icon: 'search' }] : []),
    { id: 'kind', label: 'Only this kind', icon: 'search' },
  ];
  protected readonly appBoard = computed(() => (this.store.overview()?.apps ?? []).slice(0, 5));
  protected readonly appKey = (a: AppTally): string => a.name;
  protected readonly kindKey = (k: KindTally): string => k.kind;
  protected readonly appColumns: PdColumn<AppTally>[] = [
    { key: 'name', header: 'App', value: (a) => a.name, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'count', header: 'Issues', value: (a) => a.count, align: 'end', sortable: false, width: '80px' },
    { key: 'last', header: 'Latest', value: (a) => a.last, text: (a) => this.ago(a.last), align: 'end', sortable: false, width: '100px' },
  ];
  protected readonly kindColumns: PdColumn<KindTally>[] = [
    { key: 'kind', header: 'Kind', value: (k) => ISSUE_KIND_LABEL[k.kind], required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'count', header: 'Issues', value: (k) => k.count, align: 'end', sortable: false, width: '80px' },
  ];
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

  protected select(i: Issue): void {
    this.selectedId.set(i.id);
  }

  protected run({ id, row }: PdRowActionEvent<IssueLine>): void {
    this.select(row);
    if (id === 'copy') void this.copy(row);
    else if (id === 'web') this.searchWeb(row);
    else if (id === 'app') this.setFilter({ app: row.app });
    else this.setFilter({ kind: row.kind });
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
