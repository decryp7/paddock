import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { ImpactRating, StartupEntry, StartupState } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatSeconds } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LOCATION_LABEL } from '../../shared/labels';
import { Switch } from '../../shared/switch';
import { ViewPrefs } from '../../core/view-prefs';
import { HealthStore } from '../../state/health.store';
import { HealthHero } from '../dashboard/health-hero';
import { HelperGate } from '../helper/helper-gate';

const IMPACT_LABEL: Record<ImpactRating, string> = { notMeasured: 'Not measured', low: 'Low', medium: 'Medium', high: 'High' };
const IMPACT_TONE: Record<ImpactRating, 'ok' | 'warn' | 'bad' | null> = { notMeasured: null, low: 'ok', medium: 'warn', high: 'bad' };

const IMPACT_ORDER: Record<ImpactRating, number> = { notMeasured: 0, low: 1, medium: 2, high: 3 };

/** The setting a startup entry is in, for the segmented control. */
export function startupChoice(s: StartupState): 'enabled' | 'delayed' | 'disabled' {
  return typeof s === 'string' ? s : 'delayed';
}

/** Health: the score and its reasons, and the startup manager. Changes happen only on a click and are logged and undoable. Tuning suggestions have their own page. */
@Component({
  selector: 'dy-health-page',
  imports: [AsyncState, HealthHero, Icon, RouterLink, Switch],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Health</h1>
          <p>A score with reasons, and what slows your sign-in. Dyno changes nothing until you click.</p>
        </div>
        <a class="btn" routerLink="/tuning"><dy-icon name="sparkle" />Tuning suggestions</a>
      </header>

      <dy-health-hero />

      <section class="card" aria-label="Startup manager">
        <div class="card-head">
          <h2><dy-icon name="power" /> Startup manager</h2>
          <span class="spacer"></span>
          <div class="filters" role="group" aria-label="Hide from the list">
            <button type="button" class="tog" [attr.aria-pressed]="filter().hideOs" (click)="toggle('hideOs')"><dy-icon name="check" /> Hide system entries <span class="num">{{ store.startupCounts().os }}</span></button>
            <button type="button" class="tog" [attr.aria-pressed]="filter().hideSigned" (click)="toggle('hideSigned')"><dy-icon name="check" /> Hide signed apps <span class="num">{{ store.startupCounts().signed }}</span></button>
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
                  <td class="nowrap">{{ where(e) }}@if (e.needsAdmin) { <dy-icon class="shield" name="shield" label="Changing it needs the Dyno helper" /> }</td>
                  <td>
                    <span class="signin">
                      <dy-switch [checked]="choice(e) !== 'disabled'" [label]="'Start ' + e.name + ' at sign-in'" (changed)="set(e, $event ? 'enabled' : 'disabled')" />
                      @if (choice(e) === 'delayed') {
                        <button type="button" class="btn sm ghost" [disabled]="busy(e.id)" [attr.aria-label]="'Start ' + e.name + ' without a delay'" (click)="set(e, 'enabled')">Delayed · undo</button>
                      } @else if (choice(e) === 'enabled') {
                        <button type="button" class="btn sm ghost" [disabled]="busy(e.id)" [attr.aria-label]="'Delay ' + e.name + ' at sign-in'" (click)="set(e, { delayed: 60 })">Delay</button>
                      } @else {
                        <small class="faint">Off</small>
                      }
                    </span>
                  </td>
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
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .table { margin: 0 var(--sp-4) var(--sp-3); width: calc(100% - 2 * var(--sp-4)); }
    .nowrap { white-space: nowrap; }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); background: var(--border-strong); }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .faint { color: var(--text-3); }
    .signin { display: inline-flex; align-items: center; gap: var(--sp-3); }
    .filters { display: flex; gap: var(--sp-2); }
    .tog { display: inline-flex; align-items: center; gap: 4px; height: var(--control-h); padding: 0 10px; border: 1px solid var(--border-strong); border-radius: var(--r-pill); background: var(--surface-2); color: var(--text-2); font: var(--fw-medium) var(--fs-sm) / 1 var(--font-sans); cursor: pointer; }
    .tog dy-icon { --icon-size: 13px; display: none; }
    .tog .num { color: var(--text-3); }
    .tog[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    .tog[aria-pressed='true'] dy-icon { display: inline-block; }
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
  private readonly gate = inject(HelperGate);
  protected readonly bytes = formatBytes;
  protected readonly secs = formatSeconds;
  protected readonly choice = (e: StartupEntry) => startupChoice(e.state);
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
  protected busy(id: string): boolean {
    return this.store.busy().includes(id);
  }

  protected async set(e: StartupEntry, state: StartupState): Promise<void> {
    if (startupChoice(e.state) === startupChoice(state)) return;
    // An item that starts for every user is changed by the helper: make sure it is installed first.
    if (e.needsAdmin && !(await this.gate.ensure())) return;
    await this.store.setStartup(e.id, state);
  }

}
