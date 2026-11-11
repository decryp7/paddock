import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell/contributions';
import { MatButton } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { AsyncState } from '../../shared/async-state';
import { ReadingSource } from '../../core/reading-source';
import { Icon } from '../../shared/icon';
import { EnergyStore } from '../../state/energy.store';
import { SettingsStore } from '../../state/settings.store';
import { ENERGY_RANGES, chartBars, co2Text, cpuEnergyText, costText, hasData, inputsLine, kwhText, secsText, sourceText, unknownPercent } from './energy-model';

/**
 * The energy report: how long the computer was on, asleep, or not seen by Dyno, and what that comes to from the person's own
 * numbers. The figures are labelled as an estimate; a number that was not given shows "Not set", never 0. Time Dyno was not
 * running is its own column ("Not seen"), never guessed.
 */
@Component({
  selector: 'dy-energy-tab',
  imports: [AsyncState, Icon, MatButton, MatButtonToggleModule, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!store.report()" [error]="store.error()" what="the energy report" [rows]="3" (retry)="store.load()">
      @if (store.report(); as r) {
        <section class="head" aria-label="Energy range">
          <mat-button-toggle-group aria-label="Range" [value]="store.days()" (change)="store.load($event.value)">
            @for (o of ranges; track o.days) {
              <mat-button-toggle [value]="o.days">{{ o.label }}</mat-button-toggle>
            }
          </mat-button-toggle-group>
          <button type="button" matButton="outlined" (click)="openSettings()">Open power settings</button>
        </section>
        <p class="source" role="status"><dy-icon name="info" /> {{ source() }}</p>

        @if (!r.enabled) {
          <div class="none"><dy-icon name="power" /><strong>The energy log is off</strong><span>Turn it on in power settings to record when this computer is on or asleep.</span></div>
        } @else if (!has()) {
          <div class="none"><dy-icon name="power" /><strong>Nothing recorded yet</strong><span>Dyno notes when it starts, stops, and when the computer sleeps or wakes. Come back after a day or two.</span></div>
        } @else {
          <section class="totals" aria-label="Totals for the range">
            <div><span class="k">On</span><strong>{{ secs(r.totals.onSecs) }}</strong><small>&nbsp;</small></div>
            <div><span class="k">Asleep</span><strong>{{ secs(r.totals.sleepSecs) }}</strong><small>&nbsp;</small></div>
            <div><span class="k">Not seen</span><strong>{{ secs(r.totals.unknownSecs) }}</strong><small>{{ unknown(r.totals) }}% of the range</small></div>
            <div><span class="k">Energy</span><strong>{{ kwh(r.totals.kwh) }}</strong><small>{{ r.totals.kwh === null ? '\u00a0' : 'Estimated' }}</small></div>
            <div><span class="k">Cost</span><strong>{{ cost(r.totals.cost) }}</strong><small>{{ r.totals.cost === null ? '\u00a0' : 'Estimated' }}</small></div>
            <div><span class="k">CO₂</span><strong>{{ co2(r.totals.co2Kg) }}</strong><small>{{ r.totals.co2Kg === null ? '\u00a0' : 'Estimated' }}</small></div>
          </section>
          @if (inputs(); as line) {
            <p class="inputs">{{ line }} · <button type="button" class="link" (click)="openSettings()">Edit in Settings</button></p>
          }

          @if (cpuLine(); as line) {
            <p class="inputs" data-testid="cpu-energy">{{ line }}</p>
          }
          <dy-reading-source [via]="r.cpuVia" data-testid="cpu-energy-source" />

          <figure class="chart">
            <svg viewBox="0 0 640 230" role="img" [attr.aria-label]="chartLabel()">
              @for (t of chart().ticks; track t.text) {
                <line [attr.x1]="plot.left" [attr.x2]="plot.left + plot.width" [attr.y1]="t.y" [attr.y2]="t.y" class="grid" />
                <text [attr.x]="plot.left - 6" [attr.y]="t.y + 4" class="tick" text-anchor="end">{{ t.text }}</text>
              }
              @for (b of chart().bars; track b.day.dayStart) {
                <rect [attr.x]="b.x" [attr.y]="b.onY" [attr.width]="b.w" [attr.height]="b.onH" class="on"><title>{{ day(b.day.dayStart) }}: {{ secs(b.day.onSecs) }} on, {{ secs(b.day.sleepSecs) }} asleep</title></rect>
                <rect [attr.x]="b.x" [attr.y]="b.sleepY" [attr.width]="b.w" [attr.height]="b.sleepH" class="asleep" />
                @if (b.label) {
                  <text [attr.x]="b.x + b.w / 2" [attr.y]="plot.top + plot.height + 16" class="tick" text-anchor="middle">{{ b.label }}</text>
                }
              }
              <text x="12" [attr.y]="plot.top + plot.height / 2" class="axis" text-anchor="middle" [attr.transform]="'rotate(-90 12 ' + (plot.top + plot.height / 2) + ')'">Hours per day</text>
            </svg>
            <figcaption><span class="key on"></span> On <span class="key asleep"></span> Asleep. The rest of each day is time Dyno did not see.</figcaption>
          </figure>

          <table aria-label="Energy by day">
            <thead><tr><th scope="col">Day</th><th scope="col">On</th><th scope="col">Asleep</th><th scope="col">Not seen</th><th scope="col">Energy (estimated)</th><th scope="col">Cost</th><th scope="col">CO₂</th></tr></thead>
            <tbody>
              @for (d of newestFirst(); track d.dayStart) {
                <tr>
                  <th scope="row">{{ day(d.dayStart) }}</th>
                  <td>{{ secs(d.onSecs) }}</td>
                  <td>{{ secs(d.sleepSecs) }}</td>
                  <td>{{ secs(d.unknownSecs) }}</td>
                  <td>{{ kwh(d.kwh) }}</td>
                  <td>{{ cost(d.cost) }}</td>
                  <td>{{ co2(d.co2Kg) }}</td>
                </tr>
              }
            </tbody>
          </table>
          <p class="foot">Dyno never claims savings: these are your watts times the time it saw. On a Mac, the helper adds the CPU's own energy (and fan speeds on Intel Macs) beside the estimate.</p>
        }
      }
    </dy-async>
  `,
  styles: `
    :host { display: block; padding-top: var(--sp-5); }
    dy-async { display: grid; gap: var(--sp-4); }
    .head { display: flex; gap: var(--sp-4); align-items: center; flex-wrap: wrap; justify-content: space-between; border: 0; padding: 0; background: none; }
    .source, .inputs { margin: 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); flex-wrap: wrap; }
    .link { background: none; border: 0; padding: 0; font: inherit; color: var(--accent); cursor: pointer; text-decoration: underline; }
    .none { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-8) var(--sp-5); text-align: center; color: var(--text-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .none strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .none span { max-width: 56ch; font-size: var(--fs-sm); }
    .totals { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: var(--sp-3); }
    .totals div { display: grid; grid-template-rows: auto auto auto; align-content: start; gap: 2px; border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3); min-width: 0; }
    .k { color: var(--text-3); font-size: var(--fs-sm); }
    .totals strong { font-size: 17px; font-weight: var(--fw-semibold); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
    .totals small { color: var(--text-3); font-size: var(--fs-sm); }
    .chart { margin: 0; display: grid; gap: var(--sp-2); }
    svg { width: 100%; height: auto; }
    .grid { stroke: var(--border); stroke-width: 1; }
    .tick, .axis { fill: var(--text-3); font-size: 11px; }
    .on { fill: var(--accent); }
    .asleep { fill: var(--text-3); opacity: 0.55; }
    figcaption { color: var(--text-3); font-size: var(--fs-sm); display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .key { display: inline-block; width: 10px; height: 10px; border-radius: 2px; }
    .key.on { background: var(--accent); }
    .key.asleep { background: var(--text-3); opacity: 0.55; }
    table { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
    th, td { text-align: left; padding: 6px var(--sp-3); border-bottom: 1px solid var(--border); }
    thead th { color: var(--text-3); font-weight: var(--fw-medium); }
    tbody th { font-weight: var(--fw-medium); }
    .foot { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class EnergyTab implements OnInit {
  protected readonly store = inject(EnergyStore);
  protected readonly ranges = ENERGY_RANGES;
  protected readonly source = computed(() => (this.store.report() ? sourceText(this.store.report()!) : ''));
  protected readonly cpuLine = computed(() => (this.store.report() ? cpuEnergyText(this.store.report()!) : null));
  protected readonly has = computed(() => (this.store.report() ? hasData(this.store.report()!) : false));
  protected readonly kwh = kwhText;
  protected readonly co2 = co2Text;
  protected readonly secs = secsText;
  protected readonly unknown = unknownPercent;
  private readonly settings = inject(SettingsStore);
  private readonly ports = inject(SHELL_PORTS);
  protected readonly plot = { left: 48, top: 8, width: 580, height: 180 };
  protected readonly inputs = computed(() => {
    const e = this.settings.settings()?.power.energy;
    return e ? inputsLine(e) : null;
  });
  protected readonly chart = computed(() => chartBars(this.store.report()?.days ?? [], this.plot, (d) => new Date(d.dayStart).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })));
  protected readonly newestFirst = computed(() => [...(this.store.report()?.days ?? [])].reverse());
  protected readonly chartLabel = computed(() => `Hours on and asleep for each of the last ${this.store.report()?.days.length ?? 0} days`);

  ngOnInit(): void {
    void this.store.load();
    if (!this.settings.settings()) void this.settings.load();
  }

  protected openSettings(): void {
    void this.ports.openDialog('pd.settings');
  }

  protected cost(v: number | null): string {
    return costText(v, this.store.report()?.currency ?? null);
  }

  protected day(at: number): string {
    return new Date(at).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }
}
