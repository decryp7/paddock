import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { Consumer } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatCount, formatDayTime, formatPercent, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { SEVERITY_LABEL } from '../../shared/labels';
import { Sparkline } from '../../shared/sparkline';
import { ReportsStore } from '../../state/reports.store';
import { diskForecast, scoreChangeText, weekOptions, weekRange } from './reports-model';
import { MatButton } from '@angular/material/button';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';

/**
 * Reports: Dyno's weekly summary built by the engine from its own history (health trend, busiest apps, disk forecast, issues
 * and what changed), one week at a time, exportable as Markdown or a web page. Made on this computer and saved only where you choose.
 */
@Component({
  selector: 'dy-reports-page',
  imports: [AsyncState, Icon, Sparkline, PdTable, PdSelect, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Reports</h1>
          <p>A weekly summary from Dyno's own history. Made on this computer, saved only where you choose.</p>
        </div>
        <pd-select class="week" [options]="weekChoices()" [value]="weekValue()" ariaLabel="Week" (valueChange)="pick($event)" />
        <button type="button" matButton="outlined" [disabled]="!canExport()" (click)="store.export('markdown')">
          <dy-icon name="file" /> {{ store.exporting() === 'markdown' ? 'Saving…' : 'Export Markdown…' }}
        </button>
        <button type="button" matButton="outlined" [disabled]="!canExport()" (click)="store.export('html')">
          <dy-icon name="file" /> {{ store.exporting() === 'html' ? 'Saving…' : 'Export web page…' }}
        </button>
      </header>

      @if (store.saveError(); as e) {
        <div class="note bad" role="alert">
          <dy-icon name="alert" />
          <span class="msg">{{ e.message }}</span>
        </div>
      }
      @if (store.saved(); as s) {
        <div class="note" role="status">
          <dy-icon name="check" />
          <span>Saved ({{ bytes(s.bytes) }}) to</span>
          <code class="path" tabindex="0">{{ s.path }}</code>
        </div>
      }

      <dy-async [loaded]="!!store.report()" [error]="store.error()" [empty]="!!store.report()?.nothingToReport" emptyTitle="Nothing to report" emptyText="Dyno recorded no history or issues for this week." emptyIcon="reports" what="the report" [rows]="5" (retry)="store.load()">
        <div class="stack">
        @if (store.report(); as r) {
          <p class="range num">{{ range(r) }}</p>

          <dl class="stats num" aria-label="The week in numbers">
            <div><dt>Health score</dt><dd>{{ r.score === null ? '—' : count(r.score) }}<small>{{ change(r) }}</small></dd></div>
            <div><dt>CPU</dt><dd>{{ pct(r.avgCpu) }}<small>peak {{ pct(r.peakCpu) }}</small></dd></div>
            <div><dt>Memory in use</dt><dd>{{ pct(r.avgMemory) }}<small>on average</small></dd></div>
            <div><dt>App crashes</dt><dd>{{ count(r.crashes) }}</dd></div>
            <div><dt>Alerts</dt><dd>{{ count(r.alerts) }}</dd></div>
          </dl>

          <section class="card" aria-label="Highlights">
            <div class="card-head"><h2><dy-icon name="sparkle" /> Highlights</h2></div>
            <ul class="plain">
              @for (h of r.highlights; track $index) {
                <li>{{ h }}</li>
              }
            </ul>
          </section>

          <div class="cols">
            @if (r.scoreTrend.length > 1) {
              <section class="card" aria-label="Health score trend">
                <div class="card-head"><h2><dy-icon name="health" /> Health score</h2></div>
                <div class="spark"><dy-sparkline [values]="trend()" [max]="100" [capacity]="trend().length" [zoom]="true" label="Health score over the week" /></div>
              </section>
            }
            @if (r.topApps.length) {
              <section class="card" aria-label="Busiest apps">
                <div class="card-head"><h2><dy-icon name="processes" /> Busiest apps</h2></div>
                <div class="tablewrap">
                  <pd-table [toolbar]="false" label="Busiest apps of the week" [rows]="r.topApps" [columns]="appColumns" [rowKey]="appKey" [filterable]="false" height="window" />
                </div>
              </section>
            }
            @if (r.disks.length) {
              <section class="card" aria-label="Disks">
                <div class="card-head"><h2><dy-icon name="disk" /> Disks</h2></div>
                <ul class="rows">
                  @for (d of r.disks; track d.disk) {
                    <li><span class="k">{{ d.disk }}</span><span class="num">{{ bytes(d.free) }} free of {{ bytes(d.total) }}</span><span class="faint">{{ forecast(d) }}</span></li>
                  }
                </ul>
              </section>
            }
          </div>

          <section class="card" aria-label="Issues">
            <div class="card-head"><h2><dy-icon name="alert" /> Issues</h2><span class="faint count">{{ r.issues.length ? plural(r.issues.length, 'alert') : '' }}</span></div>
            @if (r.issues.length) {
              <ul class="events">
                @for (i of r.issues; track i.at + i.title) {
                  <li>
                    <span class="t num">{{ day(i.at) }}</span>
                    <div class="x"><strong>{{ i.title }}</strong><span>{{ severity(i) }} · {{ i.detail }}</span></div>
                  </li>
                }
              </ul>
            } @else {
              <p class="none">Nothing needed attention this week.</p>
            }
          </section>

          @if (r.hardware.length) {
            <section class="card" aria-label="Hardware">
              <div class="card-head"><h2><dy-icon name="temperature" /> Hardware</h2></div>
              <ul class="plain">
                @for (h of r.hardware; track $index) {
                  <li>{{ h }}</li>
                }
              </ul>
            </section>
          }

        }
        </div>
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .stack { display: grid; gap: var(--sp-4); min-width: 0; align-content: start; }
    .page-head { margin-bottom: 0; align-items: center; }
    .week { min-width: 200px; }
    .note { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); border: 1px solid var(--border); background: var(--surface-2); font-size: var(--fs-sm); min-width: 0; }
    .note.bad { border-color: var(--bad); }
    .note > dy-icon { --icon-size: 15px; color: var(--text-3); flex: none; }
    .note .msg { flex: 1; min-width: 0; }
    .path { font: 12px/1.4 var(--font-mono); user-select: all; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .range { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .num { font-variant-numeric: tabular-nums; }
    .stats { margin: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: var(--sp-4); padding: var(--sp-4); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); }
    .stats dt { color: var(--text-3); font-size: var(--fs-sm); }
    .stats dd { margin: 2px 0 0; font: var(--fw-semibold) var(--fs-lg) / 1.3 var(--font-display); }
    .stats small { display: block; font: var(--fw-regular) var(--fs-xs) / 1.3 var(--font-sans); color: var(--text-3); }
    .plain { margin: 0; padding: var(--sp-3) var(--sp-4) var(--sp-4) var(--sp-8); display: grid; gap: var(--sp-1); font-size: var(--fs-base); }
    .cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: var(--sp-4); align-items: start; }
    .spark { padding: var(--sp-3) var(--sp-4) var(--sp-4); --spark-h: 72px; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 240px; }
    .tablewrap { margin-top: var(--sp-2); }
    .rows { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) var(--sp-3); display: grid; }
    .rows li { display: flex; gap: var(--sp-3); align-items: baseline; padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); font-size: var(--fs-base); }
    .rows li:first-child { border-top: 0; }
    .rows .k { font-weight: var(--fw-medium); min-width: 56px; }
    .rows .faint, .faint { color: var(--text-3); font-size: var(--fs-sm); }
    .rows .faint { margin-left: auto; }
    .card-head .count { margin-left: auto; }
    .events { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) var(--sp-3); display: grid; }
    .events li { display: flex; gap: var(--sp-4); padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); font-size: var(--fs-base); }
    .events li:first-child { border-top: 0; }
    .events .t { color: var(--text-3); font-size: var(--fs-sm); min-width: 96px; white-space: nowrap; padding-top: 1px; }
    .events .x { display: grid; min-width: 0; }
    .events .x span { color: var(--text-3); font-size: var(--fs-sm); }
    .lead, .none { margin: 0; padding: var(--sp-3) var(--sp-4) 0; color: var(--text-2); font-size: var(--fs-base); }
    .none { padding-bottom: var(--sp-4); color: var(--text-3); }
  `,
})
export class ReportsPage implements OnInit {
  protected readonly store = inject(ReportsStore);
  protected readonly options = computed(() => weekOptions(this.store.latestEnd()));
  protected readonly weekChoices = computed<PdSelectOption[]>(() => this.options().map((o) => ({ value: String(o.value), label: o.label })));
  protected readonly weekValue = computed(() => String(this.store.weeksAgo()));
  protected readonly canExport = computed(() => !!this.store.report() && !this.store.exporting());
  protected readonly trend = computed(() => this.store.report()?.scoreTrend.map((p) => p.score) ?? []);
  protected readonly appKey = (a: { app: string }): string => a.app;
  protected readonly appColumns: PdColumn<Consumer>[] = [
    { key: 'app', header: 'App', value: (a) => a.app, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'cpu', header: 'CPU (average)', value: (a) => a.cpuAvg, text: (a) => formatPercent(a.cpuAvg, 0), align: 'end', sortable: false, width: '150px' },
    { key: 'memory', header: 'Memory (average)', value: (a) => a.memoryAvg, text: (a) => formatBytes(a.memoryAvg), align: 'end', sortable: false, width: '150px' },
  ];
  protected readonly bytes = formatBytes;
  protected readonly pct = (v: number) => formatPercent(v, 0);
  protected readonly plural = plural;
  protected readonly count = formatCount;
  protected readonly day = formatDayTime;
  protected readonly range = weekRange;
  protected readonly change = scoreChangeText;
  protected readonly forecast = diskForecast;
  protected readonly severity = (i: { severity: keyof typeof SEVERITY_LABEL }) => SEVERITY_LABEL[i.severity];

  ngOnInit(): void {
    void this.store.load();
  }

  protected pick(value: string): void {
    void this.store.select(Number(value));
  }
}
