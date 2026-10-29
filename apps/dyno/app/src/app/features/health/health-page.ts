import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject } from '@angular/core';
import type { ImpactRating, StartupEntry } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatSeconds } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LOCATION_LABEL, startupStateText } from '../../shared/labels';
import { ViewPrefs } from '../../core/view-prefs';
import { HealthStore } from '../../state/health.store';
import { HealthHero } from '../dashboard/health-hero';
import { MatCheckbox } from '@angular/material/checkbox';

const IMPACT_LABEL: Record<ImpactRating, string> = { notMeasured: 'Not measured', low: 'Low', medium: 'Medium', high: 'High' };
const IMPACT_TONE: Record<ImpactRating, 'ok' | 'warn' | 'bad' | null> = { notMeasured: null, low: 'ok', medium: 'warn', high: 'bad' };

const IMPACT_ORDER: Record<ImpactRating, number> = { notMeasured: 0, low: 1, medium: 2, high: 3 };

/** Health: the score and its reasons, and what starts at sign-in. Dyno only shows these; it changes nothing. */
@Component({
  selector: 'dy-health-page',
  imports: [MatCheckbox, AsyncState, HealthHero, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Health</h1>
          <p>A score with reasons, and what slows your sign-in. Dyno only looks.</p>
        </div>
      </header>

      <dy-health-hero />

      <section class="card" aria-label="Start-up items">
        <div class="card-head">
          <h2><dy-icon name="power" /> Start-up items</h2>
          <span class="spacer"></span>
          <div class="filters" role="group" aria-label="Hide from the list">
            <mat-checkbox [checked]="filter().hideOs" (change)="toggle('hideOs')">Hide system entries <span class="num">{{ store.startupCounts().os }}</span></mat-checkbox>
            <mat-checkbox [checked]="filter().hideSigned" (change)="toggle('hideSigned')">Hide signed apps <span class="num">{{ store.startupCounts().signed }}</span></mat-checkbox>
          </div>
        </div>
        <dy-async [loaded]="store.startupLoaded()" [error]="store.startupError()" [empty]="store.startup().length === 0" [emptyTitle]="emptyTitle()" [emptyText]="emptyText()" what="startup items" [rows]="4" (retry)="reload()">
          <table class="table">
            <caption class="visually-hidden">Programs that start at sign-in</caption>
            <thead><tr><th>Program</th><th>Impact</th><th>Where</th><th>At sign-in</th></tr></thead>
            <tbody>
              @for (e of sorted(); track e.id) {
                <tr>
                  <td><strong>{{ e.name }}</strong><br /><small class="faint">{{ e.publisher ?? 'Unknown publisher' }}</small></td>
                  <td class="nowrap"><span class="dot" [attr.data-tone]="tone(e)" aria-hidden="true"></span>{{ impact(e) }}@if (e.impact.rating !== 'notMeasured') { <small class="faint num"> {{ secs(e.impact.cpuSecs) }} CPU · {{ bytes(e.impact.diskBytes) }}</small> }</td>
                  <td class="nowrap">{{ where(e) }}</td>
                  <td>{{ state(e) }}</td>
                </tr>
              }
            </tbody>
          </table>
          <p class="shown">Showing {{ store.startup().length }} of {{ store.startupCounts().total }} entries@if (hidden() > 0) {, {{ hidden() }} hidden by the filter}.</p>
        </dy-async>
      </section>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); align-content: start; }
    .table { margin: 0 var(--sp-4) var(--sp-3); width: calc(100% - 2 * var(--sp-4)); }
    .nowrap { white-space: nowrap; }
    .table td.nowrap:nth-child(2) { white-space: normal; }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); background: var(--border-strong); }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .faint { color: var(--text-3); }
    .filters { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
    .shown { margin: 0 var(--sp-4) var(--sp-3); color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class HealthPage implements OnInit {
  protected readonly store = inject(HealthStore);
  private readonly prefs = inject(ViewPrefs);
  protected readonly filter = this.prefs.startupFilter;
  protected readonly hidden = computed(() => this.store.startupCounts().total - this.store.startup().length);
  protected readonly emptyTitle = computed(() => (this.hidden() > 0 ? 'Everything is hidden by the filter' : 'No startup items'));
  protected readonly emptyText = computed(() =>
    this.hidden() > 0 ? 'Turn a filter off above to see the entries it hides.' : 'Nothing is set to start when you sign in.',
  );
  protected readonly bytes = formatBytes;
  protected readonly secs = formatSeconds;
  protected readonly state = (e: StartupEntry) => startupStateText(e.state);
  protected readonly impact = (e: StartupEntry) => IMPACT_LABEL[e.impact.rating];
  protected readonly tone = (e: StartupEntry) => IMPACT_TONE[e.impact.rating];
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
