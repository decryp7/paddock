import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject } from '@angular/core';
import { isNotBuilt } from '../../core/command-error';
import { ReadingSource } from '../../core/reading-source';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { ImpactRating, StartupEntry } from '../../core/api.types';
import { FullTextTip } from '../../shared/full-text-tip';
import { AsyncState } from '../../shared/async-state';
import { formatSeconds, formatStorage } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LOCATION_LABEL, startupStateText } from '../../shared/labels';
import { ViewPrefs } from '../../core/view-prefs';
import { HealthStore } from '../../state/health.store';
import { HealthHero } from '../dashboard/health-hero';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatTooltip } from '@angular/material/tooltip';

const IMPACT_LABEL: Record<ImpactRating, string> = { notMeasured: 'Not measured', low: 'Low', medium: 'Medium', high: 'High' };
const IMPACT_TONE: Record<ImpactRating, 'ok' | 'warn' | 'bad' | null> = { notMeasured: null, low: 'ok', medium: 'warn', high: 'bad' };

const IMPACT_ORDER: Record<ImpactRating, number> = { notMeasured: 0, low: 1, medium: 2, high: 3 };

/** Health: the score and its reasons, and what starts at sign-in. Dyno only shows these; it changes nothing. */
@Component({
  selector: 'dy-health-page',
  imports: [FullTextTip, MatTooltip, MatCheckbox, AsyncState, HealthHero, Icon, PdTable, PdCell, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Health</h1>
          <p>A score with reasons, and what slows your sign-in. This page only looks.</p>
        </div>
      </header>

      <section class="card" aria-label="Health score">
        <dy-async [loaded]="store.report() !== null" [error]="store.reportError()" [empty]="false" what="health score" [rows]="3" (retry)="store.loadReport()">
          <div class="hero-body"><dy-health-hero /></div>
        </dy-async>
      </section>

      <section class="card" aria-label="Start-up items">
        <div class="card-head">
          <h2><dy-icon name="power" /> Start-up items</h2>
          <span class="spacer"></span>
          <div class="filters" role="group" aria-label="Hide from the list">
            <mat-checkbox [checked]="filter().hideOs" (change)="toggle('hideOs')">Hide system entries <span class="num">{{ store.startupCounts().os }}</span></mat-checkbox>
            <mat-checkbox [checked]="filter().hideSigned" (change)="toggle('hideSigned')">Hide signed apps <span class="num">{{ store.startupCounts().signed }}</span></mat-checkbox>
          </div>
        </div>
        @if (startupGap(); as gap) {
          <p class="gap" role="status" data-testid="startup-not-read"><dy-icon name="info" /> {{ gap.message }}</p>
        } @else {
        <dy-async [loaded]="store.startupLoaded()" [error]="store.startupError()" [empty]="store.startup().length === 0" [emptyTitle]="emptyTitle()" [emptyText]="emptyText()" what="startup items" [rows]="4" (retry)="reload()">
          <div class="tablewrap">
            <pd-table [toolbar]="false" label="Programs that start at sign-in" [rows]="sorted()" [columns]="columns" [rowKey]="rowKey" [filterable]="false" [rowHeight]="36" height="window">
              <ng-template pdCell="name" let-e><span class="two"><strong class="nm" [dyFullText]="e.name">{{ e.name }}</strong><small class="faint">{{ e.publisher ?? 'Unknown publisher' }}</small></span></ng-template>
              <ng-template pdCell="impact" let-e><span class="impact" [matTooltip]="impactFull(e)"><span class="dot" [attr.data-tone]="tone(e)" aria-hidden="true"></span><span class="lbl">{{ impact(e) }}</span>@if (e.impact.rating !== 'notMeasured') { <small class="faint num ell">{{ secs(e.impact.cpuSecs) }} CPU · {{ bytes(e.impact.diskBytes) }}</small> }</span></ng-template>
            </pd-table>
          </div>
          <dy-reading-source [via]="store.startupBtmVia()" [canRetry]="true" (retry)="reload()" />
          @for (u of store.startupUnavailable(); track u) {
            @if (u !== btmReason()) { <p class="shown" role="status">Not shown: {{ u }}</p> }
          }
          <p class="shown">Showing {{ store.startup().length }} of {{ store.startupCounts().total }} entries@if (hidden() > 0) {, {{ hidden() }} hidden by the filter}.</p>
        </dy-async>
        }
      </section>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); align-content: start; }
    .hero-body { padding: var(--sp-5); }
    .tablewrap { margin: 0 var(--sp-4) var(--sp-3); }
    .two { display: grid; min-width: 0; line-height: 1.25; }
    .two > * { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .two small { font-size: var(--fs-xs); }
    /* The measured numbers ellipsise (the whole text is the tooltip); the rating and its dot never shrink. */
    .impact { display: flex; flex-wrap: wrap; align-items: center; gap: 0 var(--sp-1); min-width: 0; line-height: 1.25; }
    .impact small { flex-basis: 100%; }
    .lbl { flex: none; }
    .ell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .nowrap { white-space: nowrap; }
    .table td.nowrap:nth-child(2) { white-space: normal; }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); background: var(--border-strong); }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .faint { color: var(--text-3); }
    .filters { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
    .gap { display: flex; align-items: center; gap: var(--sp-2); margin: 0 var(--sp-4) var(--sp-3); color: var(--text-2); }
    .shown { margin: 0 var(--sp-4) var(--sp-3); color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class HealthPage implements OnInit {
  protected readonly store = inject(HealthStore);
  private readonly prefs = inject(ViewPrefs);
  protected readonly filter = this.prefs.startupFilter;
  /** A startup reader Dyno does not have on this system (the engine says `unsupported`): a note about coverage, not a failure to retry. */
  protected readonly startupGap = computed(() => {
    const e = this.store.startupError();
    return isNotBuilt(e) ? e : null;
  });
  protected readonly hidden = computed(() => this.store.startupCounts().total - this.store.startup().length);
  protected readonly emptyTitle = computed(() => (this.hidden() > 0 ? 'Everything is hidden by the filter' : 'No startup items'));
  protected readonly emptyText = computed(() =>
    this.hidden() > 0 ? 'Turn a filter off above to see the entries it hides.' : 'Nothing is set to start when you sign in.',
  );
  /** The reason the reading source already gives for the login items, so the list's own note does not say it twice. */
  protected readonly btmReason = computed((): string | null => {
    const v = this.store.startupBtmVia();
    if (typeof v !== 'object') return null;
    return 'needsHelper' in v ? v.needsHelper.reason : v.unavailable.reason;
  });
  protected readonly bytes = formatStorage;
  protected readonly secs = formatSeconds;
  protected readonly state = (e: StartupEntry) => startupStateText(e.state);
  protected readonly impact = (e: StartupEntry) => IMPACT_LABEL[e.impact.rating];
  protected readonly impactFull = (e: StartupEntry): string => (e.impact.rating === 'notMeasured' ? IMPACT_LABEL[e.impact.rating] : `${IMPACT_LABEL[e.impact.rating]}: ${this.secs(e.impact.cpuSecs)} CPU · ${this.bytes(e.impact.diskBytes)}`);
  protected readonly tone = (e: StartupEntry) => IMPACT_TONE[e.impact.rating];
  protected readonly rowKey = (e: StartupEntry): string => e.id;
  protected readonly columns: PdColumn<StartupEntry>[] = [
    { key: 'name', header: 'Program', value: (e) => e.name, required: true, width: 'minmax(0, 2fr)' },
    { key: 'impact', header: 'Impact', value: (e) => IMPACT_ORDER[e.impact.rating], text: (e) => IMPACT_LABEL[e.impact.rating], width: 'minmax(150px, 1.5fr)' },
    { key: 'where', header: 'Where', value: (e) => LOCATION_LABEL[e.location], width: 'minmax(0, 1fr)' },
    { key: 'state', header: 'At sign-in', value: (e) => startupStateText(e.state), width: '120px' },
  ];
  protected readonly where = (e: StartupEntry) => LOCATION_LABEL[e.location];

  constructor() {
    // Reload with the engine's filter whenever the person changes it (and once at the start).
    effect(() => {
      const f = this.filter();
      void this.store.loadStartup(f);
    });
  }

  ngOnInit(): void {
    void this.store.loadReport();
  }

  protected reload(): void {
    void this.store.loadStartup(this.filter());
  }

  protected toggle(key: 'hideOs' | 'hideSigned'): void {
    const f = this.filter();
    this.prefs.setStartupFilter({ ...f, [key]: !f[key] });
  }

  /** Highest startup impact first, then by name, so what slows sign-in most is on top. */
  protected readonly sorted = computed(() => [...this.store.startup()].sort((a, b) => IMPACT_ORDER[b.impact.rating] - IMPACT_ORDER[a.impact.rating] || a.name.localeCompare(b.name)));
}
