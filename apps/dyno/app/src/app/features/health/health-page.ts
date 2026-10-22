import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import type { ImpactRating, StartupEntry, StartupState } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatSeconds } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LOCATION_LABEL } from '../../shared/labels';
import { Switch } from '../../shared/switch';
import { HealthStore } from '../../state/health.store';
import { HealthHero } from '../dashboard/health-hero';

const IMPACT_LABEL: Record<ImpactRating, string> = { notMeasured: 'Not measured', low: 'Low', medium: 'Medium', high: 'High' };
const IMPACT_TONE: Record<ImpactRating, 'ok' | 'warn' | 'bad' | null> = { notMeasured: null, low: 'ok', medium: 'warn', high: 'bad' };

/** The setting a startup entry is in, for the segmented control. */
export function startupChoice(s: StartupState): 'enabled' | 'delayed' | 'disabled' {
  return typeof s === 'string' ? s : 'delayed';
}

/** Health and tuning: the score and its reasons, reversible tuning suggestions and the startup manager. Changes happen only on a click and are logged and undoable. */
@Component({
  selector: 'dy-health-page',
  imports: [AsyncState, HealthHero, Icon, Switch],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Health &amp; tuning</h1>
          <p>A score with reasons, suggestions you can undo, and what slows your sign-in. Dyno changes nothing until you click.</p>
        </div>
      </header>

      <dy-health-hero />

      <section class="card" aria-label="Tuning suggestions">
        <div class="card-head"><h2><dy-icon name="sparkle" /> Tuning suggestions</h2></div>
        <dy-async [loaded]="store.tuningLoaded()" [error]="store.tuningError()" [empty]="store.tuning().length === 0" emptyTitle="Nothing to tune" emptyText="Dyno has no suggestions for this PC right now." what="tuning suggestions" [rows]="3" (retry)="store.loadTuning()">
          <ul class="tunes">
            @for (t of store.tuning(); track t.id) {
              <li>
                <div class="txt">
                  <strong>{{ t.title }}</strong>
                  <p>{{ t.explanation }}</p>
                  <p class="change"><span class="k">Now</span> {{ t.current }} <dy-icon name="arrow-right" /> <span class="k">Suggested</span> {{ t.proposed }}</p>
                  @if (t.needsAdmin) {
                    <p class="admin"><dy-icon name="lock" /> Changing this asks for administrator approval through the Dyno helper.</p>
                  }
                </div>
                <button type="button" class="btn" [disabled]="busy(t.id)" [attr.aria-label]="(t.applied ? 'Revert ' : 'Apply ') + t.title" (click)="store.setTuning(t.id, !t.applied)">
                  {{ t.applied ? 'Revert' : 'Apply' }}
                </button>
              </li>
            }
          </ul>
        </dy-async>
      </section>

      <section class="card" aria-label="Startup manager">
        <div class="card-head"><h2><dy-icon name="power" /> Startup manager</h2></div>
        <dy-async [loaded]="store.startupLoaded()" [error]="store.startupError()" [empty]="store.startup().length === 0" emptyTitle="No startup items" emptyText="Nothing is set to start when you sign in." what="startup items" [rows]="4" (retry)="store.loadStartup()">
          <table class="table">
            <caption class="visually-hidden">Programs that start at sign-in</caption>
            <thead><tr><th>Program</th><th>Where</th><th>Impact at sign-in</th><th>At sign-in</th></tr></thead>
            <tbody>
              @for (e of store.startup(); track e.id) {
                <tr>
                  <td><strong>{{ e.name }}</strong><br /><small class="faint">{{ e.publisher ?? 'Unknown publisher' }}</small></td>
                  <td>{{ where(e) }}@if (e.needsAdmin) { <span class="badge outline" title="Changing it needs the Dyno helper">Admin</span> }</td>
                  <td><span class="badge" [attr.data-tone]="tone(e)">{{ impact(e) }}</span>@if (e.impact.rating !== 'notMeasured') { <small class="faint num"> {{ secs(e.impact.cpuSecs) }} CPU · {{ bytes(e.impact.diskBytes) }}</small> }</td>
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
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 1000px; }
    .tunes { list-style: none; margin: 0; padding: 0 var(--sp-4) var(--sp-3); display: grid; gap: var(--sp-3); }
    .tunes li { display: flex; gap: var(--sp-4); align-items: flex-start; justify-content: space-between; padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-inset); }
    .tunes p { margin: 2px 0 0; color: var(--text-2); font-size: var(--fs-sm); }
    .change { display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .change dy-icon { --icon-size: 14px; color: var(--text-3); }
    .k { color: var(--text-3); font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: 0.04em; }
    .admin { display: flex; gap: var(--sp-1); align-items: center; color: var(--text-3) !important; }
    .admin dy-icon { --icon-size: 13px; }
    .table { margin: 0 var(--sp-4) var(--sp-3); width: calc(100% - 2 * var(--sp-4)); }
    .faint { color: var(--text-3); }
    .signin { display: inline-flex; align-items: center; gap: var(--sp-3); }
    .badge.outline { margin-left: var(--sp-2); }
  `,
})
export class HealthPage implements OnInit {
  protected readonly store = inject(HealthStore);
  protected readonly bytes = formatBytes;
  protected readonly secs = formatSeconds;
  protected readonly choice = (e: StartupEntry) => startupChoice(e.state);
  protected readonly impact = (e: StartupEntry) => IMPACT_LABEL[e.impact.rating];
  protected readonly tone = (e: StartupEntry) => IMPACT_TONE[e.impact.rating];
  protected readonly where = (e: StartupEntry) => LOCATION_LABEL[e.location];

  ngOnInit(): void {
    void this.store.loadReport();
    void this.store.loadTuning();
    void this.store.loadStartup();
  }

  protected busy(id: string): boolean {
    return this.store.busy().includes(id);
  }

  protected set(e: StartupEntry, state: StartupState): void {
    if (startupChoice(e.state) !== startupChoice(state)) void this.store.setStartup(e.id, state);
  }

}
