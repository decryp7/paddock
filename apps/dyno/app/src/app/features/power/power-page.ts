import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, effect, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MatButton } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatTabsModule } from '@angular/material/tabs';
import type { AwakeUntil } from '../../core/api.types';
import { readValue } from '../../core/measured';
import { AsyncState } from '../../shared/async-state';
import { formatClock } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { injectNow } from '../../shared/now';
import { PowerStore } from '../../state/power.store';
import { ProcessesStore } from '../../state/processes.store';
import { SettingsStore } from '../../state/settings.store';
import { EnergyTab } from './energy-tab';
import { ScheduleTab } from './schedule-tab';
import { WakeDevices } from './wake-devices';
import { SleepTab } from './sleep-tab';
import {
  AWAKE_PRESETS,
  DEFAULT_KEEP_AWAKE,
  againText,
  awakeKind,
  busyConditions,
  busyConditionsText,
  busyStatusLine,
  cantTellReason,
  clockOf,
  headline,
  holdingOf,
  keepsAwakeText,
  minuteOfDay,
  presetButtonText,
  specFor,
  startedByText,
  waitingOf,
  whyDisabled,
} from './power-model';

const SUBTITLES = [
  'Keep your computer awake when you ask. Dyno holds it for you and lets go when the time is up, the program quits or you stop it.',
  'Sleep, hibernate or shut down when you are away, with a warning and a Cancel first. Off until you turn it on.',
  'How long this computer was on and asleep, and an estimate of the energy from the numbers you give.',
  'Wake computers on your network that are asleep or off, by sending them a Wake-on-LAN packet.',
  'Wake this computer at a time you choose, and then stay awake, wake other computers or run a job.',
];

/**
 * Power. Keep awake: the agent holds the computer awake for as long as the person asked and lets go by itself; the page shows
 * the agent's state (held, paused while asleep, ended and why) and asks it to start or stop. It re-implements no rule: the
 * engine validates and decides, and a hold the OS ends comes back as its own state.
 */
@Component({
  selector: 'dy-power-page',
  imports: [AsyncState, Icon, FormsModule, MatButton, MatButtonToggleModule, MatCheckbox, MatFormFieldModule, MatInputModule, MatTabsModule, EnergyTab, SleepTab, WakeDevices, ScheduleTab],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Power</h1>
          <p>{{ subtitle() }}</p>
        </div>
      </header>

      <mat-tab-group animationDuration="0ms" aria-label="Power" [selectedIndex]="tab()" (selectedIndexChange)="tab.set($event)">
        <mat-tab label="Keep awake">
          <div class="body">
            <dy-async [loaded]="!!store.view()" [error]="store.error()" what="power" [rows]="3" (retry)="store.load()">
              @if (store.view(); as v) {
                @if (unavailable(); as why) {
                  <section class="status" aria-label="Keep awake status">
                    <h2 data-state="unavailable">Keep awake isn't available on this system</h2>
                    <p>Dyno asks the system's power service to stay awake, and it did not answer. On Linux this needs systemd-logind, which most desktops have; on macOS and Windows it needs nothing extra, so restart Dyno and try again. See Help for more.</p>
                    <details>
                      <summary>Details</summary>
                      <p class="sel">{{ why }}</p>
                    </details>
                  </section>
                } @else {
                  <section class="status" aria-label="Keep awake status" aria-live="polite">
                    <h2 [attr.data-state]="stateKey()">{{ headline() }}</h2>
                    @switch (kind()) {
                      @case ('holding') {
                        @if (holding(); as h) {
                          <dl>
                            <dt>Keeps awake</dt><dd>{{ keeps(h.spec.scope) }}</dd>
                            <dt>Started by</dt><dd>you, {{ startedBy(h.spec.until) }}</dd>
                            @if (conditions(h.spec.until); as c) {
                              <dt>Conditions</dt><dd>{{ c }}</dd>
                            }
                            <dt>Since</dt><dd>{{ clock(h.since) }}</dd>
                            @if (h.ends !== null) {
                              <dt>Ends</dt><dd>{{ clock(h.ends) }}</dd>
                            }
                          </dl>
                          @if (busyStatus(h.busy); as line) {
                            <p class="waiting">{{ line }}</p>
                          }
                          <details>
                            <summary>Details</summary>
                            <dl><dt>System reference</dt><dd class="sel">{{ h.osRef }}</dd></dl>
                          </details>
                        }
                        <p class="hint">Closing the window does not end it; quitting Dyno's agent or restarting the computer does.</p>
                        <button type="button" matButton="filled" [disabled]="store.busy()" (click)="store.stop()">Stop keeping awake</button>
                      }
                      @case ('waiting') {
                        @if (waiting(); as w) {
                          <p>{{ cap(w.why) }}. Dyno takes the hold again when the computer wakes, if the time has not run out.</p>
                          <dl>
                            <dt>Keeps awake</dt><dd>{{ keeps(w.spec.scope) }}</dd>
                            <dt>Started by</dt><dd>you, {{ startedBy(w.spec.until) }}</dd>
                            <dt>Since</dt><dd>{{ clock(w.since) }}</dd>
                            @if (w.ends !== null) {
                              <dt>Ends</dt><dd>{{ clock(w.ends) }}</dd>
                            }
                          </dl>
                        }
                        <button type="button" matButton="outlined" [disabled]="store.busy()" (click)="store.stop()">Stop keeping awake</button>
                      }
                      @case ('ended') {
                        <p>Dyno is not holding the computer awake any more. It does not turn itself on again after a restart.</p>
                        <button type="button" matButton="filled" [disabled]="store.busy() || !!blocked()" (click)="again()">{{ againLabel() }}</button>
                      }
                      @default {
                        <p>While this is off, your computer sleeps by its own settings.</p>
                      }
                    }
                  </section>

                  @if (store.actionError(); as e) {
                    <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></div>
                  }
                  @if (blocked(); as why) {
                    <div class="note" role="status"><dy-icon name="info" /><span>{{ why }}</span><button type="button" matButton="text" (click)="openSettings()">Open power settings</button></div>
                  }

                  <section class="options" aria-label="Start keeping awake">
                    <h2>{{ kind() === 'holding' || kind() === 'waiting' ? 'Change the hold' : 'Keep awake' }}</h2>
                    <p class="sub">Choosing one replaces the hold Dyno has now.</p>
                    <div class="group">
                      <mat-button-toggle-group aria-label="For how long" [value]="preset()" [disabled]="store.busy() || !!blocked()" (change)="preset.set($event.value)">
                        @for (p of presets; track p.id) {
                          <mat-button-toggle [value]="p.id">{{ p.label }}</mat-button-toggle>
                        }
                      </mat-button-toggle-group>
                      <mat-checkbox [checked]="display()" [disabled]="!!blocked()" (change)="display.set($event.checked)">Keep the display on too</mat-checkbox>
                      @if (primary()) {
                        <button type="button" matButton="filled" [disabled]="store.busy() || !!blocked()" (click)="startPreset()">{{ presetButton() }}</button>
                      } @else {
                        <button type="button" matButton="outlined" [disabled]="store.busy() || !!blocked()" (click)="startPreset()">{{ presetButton() }}</button>
                      }
                    </div>

                    <div class="group" aria-label="Until a time">
                      <mat-form-field class="time-in">
                        <mat-label>Until</mat-label>
                        <input matInput type="time" [disabled]="!!blocked()" [ngModel]="until()" (ngModelChange)="until.set($event)" aria-label="Keep awake until this time" />
                      </mat-form-field>
                      <button type="button" matButton="outlined" [disabled]="store.busy() || !!timeWhy()" (click)="startAt()">{{ minute() === null ? 'Keep awake until a time' : 'Keep awake until ' + clockText(minute()!) }}</button>
                      @if (timeWhy(); as why) {
                        <small class="why">{{ why }}</small>
                      }
                    </div>

                    <fieldset class="group busy">
                      <legend>While the computer is busy</legend>
                      <mat-checkbox [checked]="busy().cpu" [disabled]="!!blocked()" (change)="setBusy({ cpu: $event.checked })">CPU above {{ limits().cpuPct }}%</mat-checkbox>
                      <mat-checkbox [checked]="busy().network" [disabled]="!!blocked()" (change)="setBusy({ network: $event.checked })">Network above {{ kb(limits().netBytesPerS) }} KB/s</mat-checkbox>
                      <mat-checkbox [checked]="busy().disk" [disabled]="!!blocked()" (change)="setBusy({ disk: $event.checked })">Disk above {{ mb(limits().diskBytesPerS) }} MB/s</mat-checkbox>
                      <small>Dyno lets go when none of these has held for {{ limits().graceSecs }} seconds. Change the limits in power settings.</small>
                      <button type="button" matButton="outlined" [disabled]="store.busy() || !!busyWhy()" (click)="startBusy()">Keep awake while busy</button>
                      @if (busyWhy(); as why) {
                        <small class="why">{{ why }}</small>
                      }
                    </fieldset>

                    <div class="group" aria-label="While a program runs">
                      <mat-form-field class="proc-in">
                        <mat-label>While this program runs</mat-label>
                        <select matNativeControl [disabled]="!!blocked()" [ngModel]="program()" (ngModelChange)="program.set($event)" aria-label="Keep awake while this program runs">
                          <option value="">Choose a program…</option>
                          @for (p of programs(); track p.key) {
                            <option [value]="p.key">{{ p.name }} (process {{ p.pid }})</option>
                          }
                        </select>
                      </mat-form-field>
                      <button type="button" matButton="outlined" [disabled]="store.busy() || !!programWhy()" (click)="startProgram()">Keep awake while it runs</button>
                      @if (programWhy(); as why) {
                        <small class="why">{{ why }}</small>
                      }
                    </div>
                  </section>
                }
              }
            </dy-async>
          </div>
        </mat-tab>
        <mat-tab label="Sleep and shut down"><ng-template matTabContent><dy-sleep-tab /></ng-template></mat-tab>
        <mat-tab label="Energy"><ng-template matTabContent><dy-energy-tab /></ng-template></mat-tab>
        <mat-tab label="Wake devices"><ng-template matTabContent><dy-wake-devices /></ng-template></mat-tab>
        <mat-tab label="Schedule"><ng-template matTabContent><dy-schedule-tab /></ng-template></mat-tab>
      </mat-tab-group>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 880px; }
    .page-head { margin-bottom: 0; }
    .body { padding-top: var(--sp-5); }
    dy-async { display: grid; gap: var(--sp-4); }
    dy-wake-devices, dy-schedule-tab { display: block; padding-top: var(--sp-4); }
    dy-async { padding-top: var(--sp-4); }
    section { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-4); display: grid; gap: var(--sp-3); }
    h2 { margin: 0; font-size: 18px; font-weight: var(--fw-semibold); }
    .status h2[data-state='holding'] { color: var(--ok); }
    .status h2[data-state='ended'], .status h2[data-state='unavailable'] { color: var(--warn); }
    .status p, .sub, .hint { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .hint { color: var(--text-3); }
    .waiting { color: var(--text-1) !important; }
    .status button { justify-self: start; }
    dl { margin: 0; display: grid; grid-template-columns: 140px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; }
    details { font-size: var(--fs-sm); color: var(--text-2); }
    summary { cursor: pointer; }
    .sel { user-select: text; cursor: text; }
    .note { display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); flex-wrap: wrap; }
    .note.bad { color: var(--bad); }
    .group { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; padding-top: var(--sp-3); border-top: 1px solid var(--border); }
    .group:first-of-type { border-top: 0; padding-top: 0; }
    .group.busy { border-left: 0; border-right: 0; border-bottom: 0; margin: 0; padding-inline: 0; flex-direction: column; align-items: flex-start; gap: 2px; }
    .busy legend { padding: 0; font-size: var(--fs-sm); color: var(--text-2); margin-bottom: var(--sp-1); }
    .busy small { color: var(--text-3); font-size: var(--fs-sm); }
    .busy button { margin-top: var(--sp-2); }
    .why { color: var(--text-3); font-size: var(--fs-sm); }
    .time-in { width: 160px; }
    .proc-in { width: 320px; }
  `,
})
export class PowerPage implements OnInit, OnDestroy {
  protected readonly store = inject(PowerStore);
  private readonly settings = inject(SettingsStore);
  private readonly processes = inject(ProcessesStore);
  private readonly ports = inject(SHELL_PORTS);
  private readonly now = injectNow(10_000);

  protected readonly presets = AWAKE_PRESETS;
  protected readonly tab = signal(0);
  protected readonly subtitle = computed(() => SUBTITLES[this.tab()] ?? SUBTITLES[0]!);
  protected readonly preset = signal<string>('1h');
  protected readonly keep = computed(() => this.settings.settings()?.power.keepAwake ?? DEFAULT_KEEP_AWAKE);
  protected readonly display = signal(false);
  protected readonly until = signal('');
  protected readonly busy = signal({ cpu: true, network: false, disk: false });
  protected readonly program = signal('');

  /** A running "while busy" hold shows its own conditions in the form (the engine is the source), not the defaults. */
  private readonly syncBusy = effect(() => {
    const u = this.holding()?.spec.until;
    if (u && typeof u === 'object' && 'whileBusy' in u) {
      const c = u.whileBusy.conds;
      this.busy.set({ cpu: c.some((x) => 'cpu' in x), network: c.some((x) => 'network' in x), disk: c.some((x) => 'disk' in x) });
    }
  });
  protected readonly limits = computed(() => {
    const base = this.keep();
    const u = this.holding()?.spec.until;
    const out = { cpuPct: base.cpuPct, netBytesPerS: base.netBytesPerS, diskBytesPerS: base.diskBytesPerS, graceSecs: base.graceSecs };
    if (u && typeof u === 'object' && 'whileBusy' in u) {
      out.graceSecs = u.whileBusy.graceSecs;
      for (const c of u.whileBusy.conds) {
        if ('cpu' in c) out.cpuPct = c.cpu.abovePct;
        else if ('network' in c) out.netBytesPerS = c.network.aboveBytesPerS;
        else if ('disk' in c) out.diskBytesPerS = c.disk.aboveBytesPerS;
      }
    }
    return out;
  });
  /** The colour key of the headline: a busy hold that is only waiting is neutral, not the active-hold green. */
  protected readonly stateKey = computed(() => {
    const b = this.holding()?.busy;
    return this.kind() === 'holding' && b && !b.active ? 'waiting' : this.kind();
  });
  protected readonly kind = computed(() => (this.store.view() ? awakeKind(this.store.view()!.awake) : 'off'));
  protected readonly holding = computed(() => (this.store.view() ? holdingOf(this.store.view()!.awake) : null));
  protected readonly waiting = computed(() => (this.store.view() ? waitingOf(this.store.view()!.awake) : null));
  protected readonly headline = computed(() => (this.store.view() ? headline(this.store.view()!.awake, this.now()) : ''));
  /** Why keep awake cannot work on this system (the engine said it cannot tell), or null. */
  protected readonly unavailable = computed(() => (this.store.view() ? cantTellReason(this.store.view()!.caps.keepAwake) : null));
  /** Why the controls are off although the system can: the setting. */
  protected readonly blocked = computed(() => (this.keep().enabled ? null : 'Keep awake is turned off in Settings.'));
  protected readonly minute = computed(() => minuteOfDay(this.until()));
  protected readonly anyBusy = computed(() => this.busy().cpu || this.busy().network || this.busy().disk);
  protected readonly primary = computed(() => this.kind() === 'off');
  protected readonly presetButton = computed(() => presetButtonText(this.presets.find((p) => p.id === this.preset()) ?? this.presets[1]!));
  protected readonly againLabel = computed(() => againText(this.store.lastSpec()));
  protected readonly timeWhy = computed(() => whyDisabled('time', this.minute() !== null, this.blocked()));
  protected readonly busyWhy = computed(() => whyDisabled('busy', this.anyBusy(), this.blocked()));
  protected readonly programWhy = computed(() => whyDisabled('program', !!this.program(), this.blocked()));
  /** Running programs to follow, one entry per process (pid and start time identify it, never the pid alone). */
  protected readonly programs = computed(() =>
    this.processes
      .processes()
      .map((p) => ({ key: `${p.pid}`, pid: p.pid, name: p.name, started: readValue(p.started) }))
      .sort((a, b) => a.name.localeCompare(b.name) || a.pid - b.pid),
  );

  ngOnInit(): void {
    void this.store.load();
    if (!this.settings.settings()) void this.settings.load();
    this.display.set(this.keep().display);
    this.processes.start();
  }

  ngOnDestroy(): void {
    this.processes.stop();
  }

  protected clock(at: number): string {
    return formatClock(at);
  }
  protected clockText = clockOf;
  protected keeps = keepsAwakeText;
  protected startedBy = startedByText;
  protected busyStatus = busyStatusLine;
  protected conditions(u: AwakeUntil): string | null {
    return busyConditionsText(u);
  }
  protected cap(text: string): string {
    return text.charAt(0).toUpperCase() + text.slice(1);
  }
  protected kb(bytes: number): number {
    return Math.round(bytes / 1024);
  }
  protected mb(bytes: number): number {
    return Math.round(bytes / 1024 / 1024);
  }
  protected setBusy(p: Partial<{ cpu: boolean; network: boolean; disk: boolean }>): void {
    this.busy.update((b) => ({ ...b, ...p }));
  }
  protected openSettings(): void {
    void this.ports.openDialog('pd.settings');
  }

  protected start(until: AwakeUntil): void {
    void this.store.start(specFor(until, this.display()));
  }
  protected startPreset(): void {
    const p = this.presets.find((x) => x.id === this.preset());
    if (p) this.start(p.until);
  }
  protected again(): void {
    void this.store.start(this.store.lastSpec() ?? specFor({ for: { minutes: 60 } }, this.display()));
  }
  protected startAt(): void {
    const m = this.minute();
    if (m !== null) this.start({ at: { minuteOfDay: m } });
  }
  protected startBusy(): void {
    this.start({ whileBusy: { conds: busyConditions(this.busy(), this.keep()), graceSecs: this.keep().graceSecs } });
  }
  protected startProgram(): void {
    const p = this.programs().find((x) => x.key === this.program());
    if (p) this.start({ whileProcess: { process: { pid: p.pid, started: p.started } } });
  }
}
