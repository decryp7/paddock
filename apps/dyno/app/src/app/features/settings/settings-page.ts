import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { Settings, Theme, TrayItem } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { Switch } from '../../shared/switch';
import { SettingsStore } from '../../state/settings.store';
import { TrayItemsEditor } from './tray-items-editor';

/** Settings: appearance, start-up, sampling and retention, the tray, alert thresholds, the observe-only posture, plugins and About. */
@Component({
  selector: 'dy-settings-page',
  imports: [FormsModule, Switch, AsyncState, Icon, TrayItemsEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Settings</h1>
          <p>Dyno measures first. Everything that changes your PC is off until you turn it on.</p>
        </div>
        @if (dirty()) {
          <button type="button" class="btn" (click)="revert()">Discard changes</button>
          <button type="button" class="btn primary" [disabled]="store.saving()" (click)="save()">Save changes</button>
        }
      </header>

      <dy-async [loaded]="!!draft()" [error]="store.error()" what="settings" [rows]="4" (retry)="store.load()">
        @if (draft(); as d) {
          <div class="stack">
            <section class="card posture" aria-label="What Dyno changes">
              <span class="ic"><dy-icon name="shield-check" /></span>
              <div class="txt">
                <h2>Observe-only by default</h2>
                <p>Dyno only reads what your PC is doing. It never ends, throttles, cleans or tunes anything on its own: a change happens only when a rule you enabled fires or you click an action, and every change is logged and undoable where the OS allows.</p>
                <p>Changes that need administrator rights go through a small privileged helper. Dyno installs it the first time you approve such an action, with the operating system's own prompt; until then nothing runs elevated.</p>
              </div>
            </section>

            <section class="card" aria-label="Appearance">
              <h2>Appearance</h2>
              <div class="row">
                <div class="lbl"><strong>Theme</strong><small>Follow the system, or always dark or light.</small></div>
                <div class="seg" role="radiogroup" aria-label="Theme">
                  @for (t of themes; track t.id) {
                    <button type="button" role="radio" [attr.aria-checked]="d.theme === t.id" (click)="set({ theme: t.id })"><dy-icon [name]="t.icon" /> {{ t.label }}</button>
                  }
                </div>
              </div>
              <div class="row">
                <div class="lbl"><strong>Start Dyno at login</strong><small>The background agent keeps measuring either way once it is running.</small></div>
                <dy-switch [checked]="d.launchAtLogin" (changed)="set({ launchAtLogin: $event })" label="Start Dyno at login" />
              </div>
            </section>

            <section class="card" aria-label="Measuring">
              <h2>Measuring and history</h2>
              <div class="row">
                <div class="lbl"><strong>System sampling</strong><small>How often CPU, memory, disk and network are read. Faster costs a little more.</small></div>
                <select class="select" aria-label="System sampling interval" [ngModel]="d.sampling.intervalMs" (ngModelChange)="setSampling({ intervalMs: +$event })">
                  @for (o of systemIntervals; track o) {
                    <option [ngValue]="o">{{ every(o) }}</option>
                  }
                </select>
              </div>
              <div class="row">
                <div class="lbl"><strong>Process sampling</strong><small>How often the process list is read.</small></div>
                <select class="select" aria-label="Process sampling interval" [ngModel]="d.sampling.processIntervalMs" (ngModelChange)="setSampling({ processIntervalMs: +$event })">
                  @for (o of processIntervals; track o) {
                    <option [ngValue]="o">{{ every(o) }}</option>
                  }
                </select>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep raw samples</strong><small>Hours of one-sample-per-tick history, stored in the agent's local database.</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="72" aria-label="Hours of raw samples" [ngModel]="d.retention.rawHours" (ngModelChange)="setRetention({ rawHours: +$event })" /><span>hours</span></label>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep 1-minute roll-ups</strong><small>Older history is averaged into minutes, then quarter-hours, so it stays small.</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="30" aria-label="Days of 1-minute roll-ups" [ngModel]="d.retention.minuteDays" (ngModelChange)="setRetention({ minuteDays: +$event })" /><span>days</span></label>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep 15-minute roll-ups</strong><small>The longest view of how your PC behaves.</small></div>
                <label class="num-in"><input class="input" type="number" min="7" max="400" aria-label="Days of 15-minute roll-ups" [ngModel]="d.retention.quarterHourDays" (ngModelChange)="setRetention({ quarterHourDays: +$event })" /><span>days</span></label>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep hourly roll-ups</strong><small>Hourly averages with the minimum and maximum, for the 30-day view and beyond.</small></div>
                <label class="num-in"><input class="input" type="number" min="30" max="1100" aria-label="Days of hourly roll-ups" [ngModel]="d.retention.hourDays" (ngModelChange)="setRetention({ hourDays: +$event })" /><span>days</span></label>
              </div>
            </section>

            <section class="card" aria-label="Menu bar items">
              <h2>Menu bar items</h2>
              <p class="sub">What the tray / menu-bar icon draws live. Add up to four, drag to reorder; the preview follows your choices.</p>
              <dy-tray-items-editor [items]="d.trayItems?.items ?? []" (itemsChange)="setTrayItems($event)" />
            </section>

            <section class="card" aria-label="Alerts">
              <h2>Alerts</h2>
              <div class="row">
                <div class="lbl"><strong>Tell me about problems</strong><small>Low disk space, overheating and long high CPU use. Alerts only inform.</small></div>
                <dy-switch [checked]="d.alerts.enabled" (changed)="setAlerts({ enabled: $event })" label="Alerts" />
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Low disk space</strong><small>Alert when a drive has less than this much free.</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="50" [disabled]="!d.alerts.enabled" aria-label="Low disk space percent" [ngModel]="d.alerts.lowDiskPercent" (ngModelChange)="setAlerts({ lowDiskPercent: +$event })" /><span>% free</span></label>
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Overheating</strong><small>Alert when a sensor passes this temperature.</small></div>
                <label class="num-in"><input class="input" type="number" min="60" max="120" [disabled]="!d.alerts.enabled" aria-label="Overheating temperature" [ngModel]="d.alerts.overheatCelsius" (ngModelChange)="setAlerts({ overheatCelsius: +$event })" /><span>°C</span></label>
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Long high CPU</strong><small>Alert when one app keeps the CPU busy this long.</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="240" [disabled]="!d.alerts.enabled" aria-label="Minutes of high CPU" [ngModel]="d.alerts.longHighCpuMinutes" (ngModelChange)="setAlerts({ longHighCpuMinutes: +$event })" /><span>minutes</span></label>
              </div>
            </section>
          </div>
        }
      </dy-async>

      <section class="card" aria-label="Plugins">
        <h2>Plugins</h2>
        <dy-async [loaded]="store.pluginsLoaded()" [error]="store.pluginsError()" [empty]="store.plugins().length === 0" emptyTitle="No plugins installed" emptyText="Plugins run in their own process and can never take Dyno down." what="plugins" [rows]="2" (retry)="store.loadPlugins()">
          <ul class="plugins">
            @for (p of store.plugins(); track p.id) {
              <li>
                <span class="dot" [attr.data-tone]="pluginTone(p.state)"></span>
                <div class="txt"><strong>{{ p.name }} <small class="num">{{ p.version }}</small></strong><small>{{ p.error ?? (p.contributes.join(', ') || 'Nothing contributed yet') }}</small></div>
                <span class="badge" [attr.data-tone]="pluginTone(p.state)">{{ pluginState(p.state) }}</span>
              </li>
            }
          </ul>
        </dy-async>
      </section>

      <section class="card about" aria-label="About">
        <h2>About</h2>
        @if (store.appInfo(); as a) {
          <dl>
            <dt>Version</dt><dd class="num">{{ a.version }}</dd>
            <dt>Platform</dt><dd>{{ osName(a.os) }}</dd>
            <dt>Updates</dt>
            <dd>{{ a.updatesAvailable ? 'An update is available.' : (a.updatesReason ?? 'Dyno is up to date.') }}</dd>
          </dl>
        } @else {
          <div class="skeleton" style="width: 40%"></div>
        }
      </section>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { max-width: 860px; display: grid; gap: var(--sp-4); }
    .page-head { margin-bottom: 0; }
    .stack { display: grid; gap: var(--sp-4); }
    .card { padding: var(--sp-4) var(--sp-5); }
    .card > h2 { margin-bottom: var(--sp-2); }
    .row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    .row.off { opacity: 0.55; }
    .lbl { display: grid; gap: 2px; min-width: 0; }
    .lbl small, .txt small { color: var(--text-3); font-size: var(--fs-sm); }
    .num-in { display: inline-flex; align-items: center; gap: var(--sp-2); color: var(--text-3); white-space: nowrap; }
    .num-in .input { width: 80px; text-align: right; }
    .sub { color: var(--text-3); margin-bottom: var(--sp-3); font-size: var(--fs-sm); }
    .checks { display: flex; gap: var(--sp-3); flex-wrap: wrap; justify-content: flex-end; }
    .check { display: inline-flex; align-items: center; gap: 6px; }
    .posture { display: flex; gap: var(--sp-4); border-color: color-mix(in srgb, var(--ok) 30%, var(--border)); }
    .posture .ic { display: grid; place-items: center; width: 36px; height: 36px; flex: none; border-radius: var(--r-md); background: var(--ok-soft); color: var(--ok); }
    .posture .txt { display: grid; gap: var(--sp-2); }
    .posture p { color: var(--text-2); max-width: 70ch; }
    .plugins { list-style: none; margin: 0; padding: 0; display: grid; }
    .plugins li { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); }
    .plugins .txt { flex: 1; display: grid; }
    .about dl { margin: 0; display: grid; grid-template-columns: 110px 1fr; gap: var(--sp-2); }
    .about dt { color: var(--text-3); } .about dd { margin: 0; }
    @media (max-width: 760px) { .row { flex-direction: column; align-items: flex-start; gap: var(--sp-2); } }
  `,
})
export class SettingsPage implements OnInit {
  protected readonly store = inject(SettingsStore);
  protected readonly draft = signal<Settings | null>(null);
  protected readonly themes: { id: Theme; label: string; icon: 'monitor' | 'moon' | 'sun' }[] = [
    { id: 'system', label: 'System', icon: 'monitor' },
    { id: 'dark', label: 'Dark', icon: 'moon' },
    { id: 'light', label: 'Light', icon: 'sun' },
  ];
  protected readonly systemIntervals = [500, 1000, 2000, 5000];
  protected readonly processIntervals = [1000, 2000, 5000, 10000];
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly dirty = computed(() => {
    const d = this.draft();
    const s = this.store.settings();
    return d !== null && s !== null && JSON.stringify(d) !== JSON.stringify(s);
  });

  constructor() {
    // A fresh copy whenever the saved settings change (first load, after saving).
    effect(() => {
      const s = this.store.settings();
      untracked(() => this.draft.set(s ? structuredClone(s) : null));
    });
  }

  ngOnInit(): void {
    void this.store.load();
    void this.store.loadPlugins();
  }

  protected set(patch: Partial<Settings>): void {
    this.draft.update((d) => (d ? { ...d, ...patch } : d));
    // The theme applies at once so the choice is visible; it is stored with Save.
    if (patch.theme) this.store.applyTheme(patch.theme);
  }
  protected setSampling(p: Partial<Settings['sampling']>): void {
    this.draft.update((d) => (d ? { ...d, sampling: { ...d.sampling, ...p } } : d));
  }
  protected setRetention(p: Partial<Settings['retention']>): void {
    this.draft.update((d) => (d ? { ...d, retention: { ...d.retention, ...p } } : d));
  }
  protected setAlerts(p: Partial<Settings['alerts']>): void {
    this.draft.update((d) => (d ? { ...d, alerts: { ...d.alerts, ...p } } : d));
  }
  protected setTrayItems(items: TrayItem[]): void {
    this.draft.update((d) => (d ? { ...d, trayItems: { version: d.trayItems?.version ?? 1, items } } : d));
  }
  protected async save(): Promise<void> {
    const d = this.draft();
    if (d) await this.store.save(d);
  }
  protected revert(): void {
    const s = this.store.settings();
    this.draft.set(s ? structuredClone(s) : null);
    if (s) this.store.applyTheme(s.theme);
  }

  protected every(ms: number): string {
    return ms < 1000 ? 'Every half second' : ms === 1000 ? 'Every second' : `Every ${ms / 1000} seconds`;
  }
  protected readonly osName = (os: string) => (os === 'macos' ? 'macOS' : os === 'windows' ? 'Windows' : 'Linux');
  protected readonly pluginState = (s: string) => (s === 'running' ? 'Running' : s === 'stopped' ? 'Stopped' : s === 'failed' ? 'Failed' : 'Paused after failures');
  protected readonly pluginTone = (s: string) => (s === 'running' ? 'ok' : s === 'stopped' ? 'muted' : 'bad');
}
