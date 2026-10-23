import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { ImpactRating, StartupEntry, StartupState } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatSeconds } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LOCATION_LABEL } from '../../shared/labels';
import { Switch } from '../../shared/switch';
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
        <div class="card-head"><h2><dy-icon name="power" /> Startup manager</h2></div>
        <dy-async [loaded]="store.startupLoaded()" [error]="store.startupError()" [empty]="store.startup().length === 0" emptyTitle="No startup items" emptyText="Nothing is set to start when you sign in." what="startup items" [rows]="4" (retry)="store.loadStartup()">
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
  `,
})
export class HealthPage implements OnInit {
  protected readonly store = inject(HealthStore);
  private readonly gate = inject(HelperGate);
  protected readonly bytes = formatBytes;
  protected readonly secs = formatSeconds;
  protected readonly choice = (e: StartupEntry) => startupChoice(e.state);
  protected readonly impact = (e: StartupEntry) => IMPACT_LABEL[e.impact.rating];
  protected readonly tone = (e: StartupEntry) => IMPACT_TONE[e.impact.rating];
  protected readonly where = (e: StartupEntry) => LOCATION_LABEL[e.location];

  ngOnInit(): void {
    void this.store.loadReport();
    void this.store.loadStartup();
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
