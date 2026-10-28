import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { BackupSettings, BackupTool, BaselineSettings, HardwareAlarms, IpLookupSettings, Settings, Theme, TrayItem, UnusualSettings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { errorHint, toCommandError } from '../../core/command-error';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { Switch } from '../../shared/switch';
import { SettingsStore } from '../../state/settings.store';
import { LOOKUP_PRESETS, presetIdFor, templateProblem } from './ip-lookup-presets';
import { TrayItemsEditor } from './tray-items-editor';
import { HelperCard } from '../helper/helper-card';
import { BACKUP_TOOLS, OVERRIDABLE, folderProblem, overrideText, validateBackup, withOverride } from '../backup/backup-model';

/** Settings: appearance, start-up, sampling and retention, the tray, alert thresholds, the observe-only posture, plugins and About. */
@Component({
  selector: 'dy-settings-page',
  imports: [FormsModule, Switch, AsyncState, Icon, TrayItemsEditor, HelperCard],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page narrow">
      <header class="page-head">
        <div class="titles">
          <h1>Settings</h1>
          <p>Dyno only looks. It changes nothing on your PC.</p>
        </div>
        @if (store.saveError(); as failed) {
          <span class="save-error" role="alert">{{ failed.message }}</span>
        }
        @if (dirty()) {
          <button type="button" class="btn" (click)="revert()">Discard changes</button>
          <button type="button" class="btn primary" [disabled]="store.saving() || !!backupProblem()" (click)="save()">Save changes</button>
        }
      </header>

      <dy-async [loaded]="!!draft()" [error]="store.error()" what="settings" [rows]="4" (retry)="store.load()">
        @if (draft(); as d) {
          <div class="stack">
            <p class="posture" aria-label="What Dyno changes"><dy-icon name="shield-check" /> Dyno only reads and changes nothing on your computer. The few readings that need administrator rights use a small read-only helper that is installed only when you approve it.</p>

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

            <section class="card" aria-label="IP lookup">
              <h2>Network ▸ IP lookup</h2>
              <p class="sub">“Look up IP” on the Network page opens this address in your browser with the remote address filled in. Dyno never fetches it, and only the address goes into it. Private and local addresses are never looked up.</p>
              <div class="row">
                <div class="lbl"><strong>Lookup site</strong><small>Pick one, or choose Custom and type your own address.</small></div>
                <select class="select" aria-label="Lookup site" [ngModel]="presetId()" (ngModelChange)="choosePreset($event)">
                  @for (p of presets; track p.id) {
                    <option [value]="p.id">{{ p.name }} — {{ p.about }}</option>
                  }
                  <option value="custom">Custom</option>
                </select>
              </div>
              <div class="row">
                <div class="lbl"><strong>Address</strong><small>https, with {{ '{ip}' }} where the address goes.</small></div>
                <div class="ipt">
                  <input class="input" type="text" aria-label="Lookup address" maxlength="300" [ngModel]="ipLookup().ipTemplate" (ngModelChange)="setLookup({ ipTemplate: $event })" />
                  <button type="button" class="btn sm" [disabled]="!!ipProblem()" (click)="testLookup()">Test with 8.8.8.8</button>
                </div>
                @if (ipProblem(); as p) {
                  <small class="problem" role="alert">{{ p }}</small>
                }
              </div>
              <div class="row">
                <div class="lbl"><strong>Network owner address (optional)</strong><small>Used when a network number (AS) is known. Contains {{ '{asn}' }}.</small></div>
                <div class="ipt">
                  <input class="input" type="text" aria-label="ASN lookup address" maxlength="300" [ngModel]="ipLookup().asnTemplate" (ngModelChange)="setLookup({ asnTemplate: $event })" />
                </div>
                @if (asnProblem(); as p) {
                  <small class="problem" role="alert">{{ p }}</small>
                }
              </div>
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

            <section class="card" aria-label="Hardware and baselines">
              <h2>Hardware and baselines</h2>
              <p class="sub">Dyno learns what is normal for this PC at each time of the week, and watches the CPU speed, disks and battery. These only raise alarms when you allow it.</p>
              <div class="row">
                <div class="lbl"><strong>Learn what is normal</strong><small>Shows on the dashboard what is unusual for this time of the week. Nothing is highlighted while Dyno is still learning.</small></div>
                <dy-switch [checked]="baselines().enabled" (changed)="setBaselines({ enabled: $event })" label="Learn what is normal" />
              </div>
              <div class="row" [class.off]="!baselines().enabled">
                <div class="lbl"><strong>Learning period</strong><small>Days of history before anything is called unusual (1 to 30).</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="30" [disabled]="!baselines().enabled" aria-label="Days of learning" [ngModel]="baselines().learningDays" (ngModelChange)="setLearningDays($event)" /><span>days</span></label>
              </div>
              <div class="row" [class.off]="!baselines().enabled">
                <div class="lbl"><strong>Alarm when far above usual</strong><small>Raises an alarm when something stays far above what is normal for the time of week. Off by default.</small></div>
                <dy-switch [checked]="baselines().alarms" (changed)="setBaselines({ alarms: $event })" label="Alarm when far above usual" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Alarm for throttling</strong><small>Raises an alarm when the CPU is slowed down for a while.</small></div>
                <dy-switch [checked]="hardwareAlarms().throttling" (changed)="setHardwareAlarms({ throttling: $event })" label="Alarm for throttling" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Alarm for worn disks and batteries</strong><small>Raises an alarm when a disk or battery is near the end of its life.</small></div>
                <dy-switch [checked]="hardwareAlarms().lifespan" (changed)="setHardwareAlarms({ lifespan: $event })" label="Alarm for disks and batteries near end of life" />
              </div>
            </section>

            <section class="card" aria-label="New programs">
              <h2>New programs</h2>
              <p class="sub">Dyno notes which programs run on this PC and lists new ones on the Issues page, with the reasons some may be worth a look. It only watches: it never ends or removes a program, and it says "unusual because", never "malware".</p>
              <div class="row">
                <div class="lbl"><strong>Watch for new programs</strong><small>Keeps a list of the programs it has seen, on this PC only.</small></div>
                <dy-switch [checked]="unusual().enabled" (changed)="setUnusual({ enabled: $event })" label="Watch for new programs" />
              </div>
              <div class="row" [class.off]="!unusual().enabled">
                <div class="lbl"><strong>Learn quietly first</strong><small>For this long after the first look, nothing is listed, so a fresh install does not flag everything it already has.</small></div>
                <label class="num-in"><input class="input" type="number" min="0" max="60" [disabled]="!unusual().enabled" aria-label="Days of quiet learning" [ngModel]="unusual().learningDays" (ngModelChange)="setUnusual({ learningDays: +$event })" /><span>days</span></label>
              </div>
            </section>

            <section class="card" aria-label="Backup health" id="backup">
              <h2>Backup health</h2>
              <p class="sub">Dyno reads the state of Time Machine, Windows Backup, File History, restic, Borg and Duplicati on the Backup health page. It never runs or manages a backup.</p>
              <div class="row">
                <div class="lbl"><strong>Tell me when a backup is out of date</strong><small>Checks every 6 hours, inside the maintenance windows, and raises an alarm.</small></div>
                <dy-switch [checked]="backup().alertWhenStale" (changed)="setBackup({ alertWhenStale: $event })" label="Tell me when a backup is out of date" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Out of date after</strong><small>A backup older than this counts as stale (1 hour to 90 days).</small></div>
                <label class="num-in"><input class="input" type="number" min="1" max="2160" aria-label="Hours until a backup is out of date" [ngModel]="backup().staleAfterHours" (ngModelChange)="setBackup({ staleAfterHours: +$event })" /><span>hours</span></label>
              </div>
              @for (t of overridable; track t.tool) {
                <div class="row top">
                  <div class="lbl"><strong>Where {{ t.name }} is installed</strong><small>Only if Dyno cannot find it. Full folder paths, one per line; searched before the usual places.</small></div>
                  <div class="ipt">
                    <textarea class="input" rows="2" spellcheck="false" [attr.aria-label]="t.name + ' folders'" [ngModel]="overrideValue(t.tool)" (ngModelChange)="setOverride(t.tool, $event)"></textarea>
                    @if (overrideProblem(t.tool); as p) {
                      <small class="problem" role="alert">{{ p }}</small>
                    }
                  </div>
                </div>
              }
            </section>
          </div>
        }
      </dy-async>

      <dy-helper-card />

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
    .save-error { color: var(--bad); font-size: 12px; align-self: center; }
    .page { display: grid; gap: var(--sp-4); }
    .page-head { margin-bottom: 0; }
    .stack { display: grid; gap: var(--sp-4); }
    .card { padding: var(--sp-4) var(--sp-5); }
    .card > h2 { margin-bottom: var(--sp-2); }
    .row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    .row.off { opacity: 0.55; }
    .ipt { display: flex; gap: var(--sp-2); flex: 1 1 380px; max-width: 560px; }
    .row.top { align-items: flex-start; }
    .ipt:has(textarea) { flex-direction: column; gap: var(--sp-1); }
    textarea.input { resize: vertical; min-height: 56px; font: var(--fw-regular) 12px / 1.4 var(--font-mono); padding: var(--sp-2) var(--sp-3); height: auto; }
    .ipt .input { flex: 1; min-width: 0; }
    .problem { color: var(--bad); font-size: var(--fs-sm); }
    .lbl { display: grid; gap: 2px; min-width: 0; }
    .lbl small, .txt small { color: var(--text-3); font-size: var(--fs-sm); }
    .num-in { display: inline-flex; align-items: center; gap: var(--sp-1); height: var(--control-h); padding: 0 var(--sp-3) 0 var(--sp-1); border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-3); white-space: nowrap; }
    .num-in:focus-within { border-color: var(--focus); outline: 1px solid var(--focus); }
    .num-in .input { width: 56px; height: 100%; padding: 0 var(--sp-1); border: 0; background: none; outline: none; text-align: right; font-variant-numeric: tabular-nums; }
    .sub { color: var(--text-3); margin-bottom: var(--sp-3); font-size: var(--fs-sm); }
    .checks { display: flex; gap: var(--sp-3); flex-wrap: wrap; justify-content: flex-end; }
    .check { display: inline-flex; align-items: center; gap: 6px; }
    .posture { display: flex; gap: var(--sp-2); align-items: flex-start; margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .posture dy-icon { --icon-size: 16px; color: var(--text-3); flex: none; margin-top: 1px; }
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
  private readonly backend = inject(BACKEND);
  private readonly notifier = inject(Notifier);
  protected readonly draft = signal<Settings | null>(null);
  protected readonly themes: { id: Theme; label: string; icon: 'monitor' | 'moon' | 'sun' }[] = [
    { id: 'system', label: 'System', icon: 'monitor' },
    { id: 'dark', label: 'Dark', icon: 'moon' },
    { id: 'light', label: 'Light', icon: 'sun' },
  ];
  protected readonly presets = LOOKUP_PRESETS;
  protected readonly ipLookup = computed(() => this.draft()?.ipLookup ?? { ipTemplate: 'https://ipinfo.io/{ip}', asnTemplate: '' });
  protected readonly presetId = computed(() => presetIdFor(this.ipLookup().ipTemplate));
  protected readonly ipProblem = computed(() => templateProblem(this.ipLookup().ipTemplate, 'ip'));
  protected readonly asnProblem = computed(() => templateProblem(this.ipLookup().asnTemplate, 'asn'));
  protected readonly baselines = computed<BaselineSettings>(() => this.draft()?.baselines ?? { enabled: true, learningDays: 7, alarms: false });
  protected readonly hardwareAlarms = computed<HardwareAlarms>(() => this.draft()?.hardwareAlarms ?? { throttling: true, lifespan: true });
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
      untracked(() => {
        this.draft.set(s ? structuredClone(s) : null);
        this.rawOverrides.set({});
      });
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
  protected setLookup(p: Partial<IpLookupSettings>): void {
    this.draft.update((d) => (d ? { ...d, ipLookup: { ...this.ipLookup(), ...p } } : d));
  }
  protected choosePreset(id: string): void {
    const p = LOOKUP_PRESETS.find((x) => x.id === id);
    if (p) this.setLookup({ ipTemplate: p.ip, asnTemplate: p.asn });
  }
  /** Opens the template as typed with 8.8.8.8 (the engine validates it and says what is wrong). */
  protected testLookup(): void {
    void this.backend.lookupIp('8.8.8.8', this.ipLookup().ipTemplate).catch((e: unknown) => this.notifier.show(toCommandError(e).message, 'bad'));
  }
  protected setSampling(p: Partial<Settings['sampling']>): void {
    this.draft.update((d) => (d ? { ...d, sampling: { ...d.sampling, ...p } } : d));
  }
  protected setRetention(p: Partial<Settings['retention']>): void {
    this.draft.update((d) => (d ? { ...d, retention: { ...d.retention, ...p } } : d));
  }
  protected readonly unusual = computed(() => this.draft()?.unusual ?? { enabled: true, learningDays: 7 });
  protected setUnusual(p: Partial<UnusualSettings>): void {
    this.draft.update((d) => (d ? { ...d, unusual: { ...(d.unusual ?? { enabled: true, learningDays: 7 }), ...p } } : d));
  }
  protected readonly backup = computed<BackupSettings>(() => this.draft()?.backup ?? { alertWhenStale: true, staleAfterHours: 48, overrides: [] });
  protected readonly overridable = BACKUP_TOOLS.filter((t) => OVERRIDABLE.includes(t.tool));
  /** What the person typed in each folder box (kept as typed, so a new line is not swallowed while they type). */
  private readonly rawOverrides = signal<Partial<Record<BackupTool, string>>>({});
  protected readonly backupProblem = computed(() => validateBackup(this.backup()));
  protected setBackup(p: Partial<BackupSettings>): void {
    this.draft.update((d) => (d ? { ...d, backup: { ...this.backup(), ...p } } : d));
  }
  protected overrideValue(tool: BackupTool): string {
    return this.rawOverrides()[tool] ?? overrideText(this.backup(), tool);
  }
  protected setOverride(tool: BackupTool, text: string): void {
    this.rawOverrides.update((r) => ({ ...r, [tool]: text }));
    this.draft.update((d) => (d ? { ...d, backup: withOverride(this.backup(), tool, text) } : d));
  }
  protected overrideProblem(tool: BackupTool): string | null {
    for (const dir of this.backup().overrides.find((o) => o.tool === tool)?.dirs ?? []) {
      const p = folderProblem(dir);
      if (p) return p;
    }
    return null;
  }
  protected setAlerts(p: Partial<Settings['alerts']>): void {
    this.draft.update((d) => (d ? { ...d, alerts: { ...d.alerts, ...p } } : d));
  }
  protected setBaselines(p: Partial<BaselineSettings>): void {
    this.draft.update((d) => (d ? { ...d, baselines: { ...this.baselines(), ...p } } : d));
  }
  /** A typed number: only a whole number from 1 to 30 is taken (the engine validates the same range). */
  protected setLearningDays(v: unknown): void {
    const n = Math.round(Number(v));
    if (Number.isFinite(n) && n >= 1 && n <= 30) this.setBaselines({ learningDays: n });
  }
  protected setHardwareAlarms(p: Partial<HardwareAlarms>): void {
    this.draft.update((d) => (d ? { ...d, hardwareAlarms: { ...this.hardwareAlarms(), ...p } } : d));
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
    this.rawOverrides.set({});
    if (s) this.store.applyTheme(s.theme);
  }

  protected every(ms: number): string {
    return ms < 1000 ? 'Every half second' : ms === 1000 ? 'Every second' : `Every ${ms / 1000} seconds`;
  }
  protected readonly osName = (os: string) => (os === 'macos' ? 'macOS' : os === 'windows' ? 'Windows' : 'Linux');
  protected readonly pluginState = (s: string) => (s === 'running' ? 'Running' : s === 'stopped' ? 'Stopped' : s === 'failed' ? 'Failed' : 'Paused after failures');
  protected readonly pluginTone = (s: string) => (s === 'running' ? 'ok' : s === 'stopped' ? 'muted' : 'bad');
}
