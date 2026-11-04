import { ChangeDetectionStrategy, Component, ElementRef, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButton } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatCheckbox } from '@angular/material/checkbox';
import { ErrorStateMatcher } from '@angular/material/core';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import type { DynoJob, WakeInput, WakeRepeat, WakeRow } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { HelperStore } from '../../state/helper.store';
import { SettingsStore } from '../../state/settings.store';
import { WakesStore } from '../../state/wakes.store';
import { WolStore } from '../../state/wol.store';
import {
  JOB_LABELS,
  WEEKDAYS,
  blankWake,
  clock,
  failedWhy,
  lastRunShort,
  minuteOf,
  repeatText,
  routeCantTell,
  routeKind,
  routeText,
  rowKind,
  statusText,
  thenText,
  wakeFieldOf,
  wakeInputFrom,
} from './schedule-model';

type Field = 'name' | 'time' | 'repeat' | 'then';

/**
 * Schedule (ADR 0032 section 5, D9). A time of day at which the computer wakes and then stays awake, wakes saved Wake-on-LAN
 * computers or runs one job from a short fixed list. The agent sets the OS wake (Windows: its own timer; macOS and the Linux package:
 * the administrator helper) and says honestly when it cannot, with the button that fixes it (Turn on the helper, Turn on scheduled
 * wake). It re-implements no rule: the agent validates and decides.
 */
@Component({
  selector: 'dy-schedule-tab',
  imports: [AsyncState, Icon, JobProgressCard, FormsModule, MatButton, MatButtonToggleModule, MatCheckbox, MatFormFieldModule, MatInputModule, MatSlideToggle, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!store.view()" [error]="store.error()" what="the schedule" [rows]="3" (retry)="store.load()">
      @if (store.view(); as v) {
        @let kind = routeKind(v.route);
        @if (!wakeEnabled()) {
          <div class="note warn" role="status">
            <dy-icon name="info" />
            <span>Scheduled wake is turned off, so Dyno sets no wake and runs nothing at a set time. Schedules you save stay paused until you turn it on.</span>
            <button type="button" matButton="filled" [disabled]="settings.saving()" (click)="turnOnWake()">Turn on scheduled wake</button>
          </div>
        } @else {
          <div class="note" [class.warn]="kind === 'needsHelper' || kind === 'unavailable'" role="status">
            <dy-icon [name]="kind === 'needsHelper' || kind === 'unavailable' ? 'alert' : 'info'" />
            <span>{{ routeText(v.route) }}</span>
            @if (kind === 'needsHelper') {
              <button type="button" matButton="filled" [disabled]="helper.action() !== null" (click)="turnOnHelper()">Turn on the helper</button>
            }
          </div>
          @if (helper.action() !== null) {
            <dy-job-progress [progress]="helper.job()" (cancel)="helper.cancel()" />
          }
          @if (helper.actionError(); as why) {
            <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ why }}</span></div>
          }
          @if (cantTell(v.route).length) {
            <ul class="cant" aria-label="What Dyno cannot tell here">
              @for (l of cantTell(v.route); track l) {
                <li><dy-icon name="info" /><span>{{ l }}</span></li>
              }
            </ul>
          }
        }

        <section aria-label="Scheduled wakes">
          <div class="head">
            <h2>Scheduled wakes</h2>
            <div class="acts">
              @if (!form()) {
                @if (addPrimary()) {
                  <button type="button" matButton="filled" (click)="openForm(blank())"><dy-icon name="plus" /> Add schedule</button>
                } @else {
                  <button type="button" matButton="outlined" [disabled]="addBlocked()" [attr.title]="addReason()" (click)="openForm(blank())"><dy-icon name="plus" /> Add schedule</button>
                }
              }
            </div>
          </div>
          @if (v.rows.length === 0 && !form()) {
            <div class="empty">
              <dy-icon name="calendar" />
              <strong>No scheduled wakes yet</strong>
              <span>Add one to wake this computer at a set time and keep it awake, wake other computers or run a job. A computer that is shut down is not woken.</span>
            </div>
          } @else if (v.rows.length > 0) {
            <pd-table [toolbar]="false" label="Scheduled wakes" [rows]="v.rows" [columns]="columns" [rowKey]="rowKey" [filterable]="false" [rowHeight]="56" [rowActions]="actions" (rowActivate)="edit($event)" (action)="run($event)">
              <ng-template pdCell="name" let-r><strong class="nm" [attr.title]="r.entry.name">{{ r.entry.name }}</strong></ng-template>
              <ng-template pdCell="when" let-r><span class="muted">{{ clock(r.entry.minuteOfDay) }}, {{ repeatText(r.entry.repeat) }}</span></ng-template>
              <ng-template pdCell="then" let-r><span class="wrap" [attr.title]="then(r)">{{ then(r) }}</span></ng-template>
              <ng-template pdCell="last" let-r>
                @if (last(r); as l) {
                  <span class="st" [attr.data-kind]="l.ok ? 'armed' : 'failed'" [attr.title]="l.text"><dy-icon [name]="l.ok ? 'check' : 'alert'" /><span class="wrap">{{ l.text }}</span></span>
                } @else {
                  <span class="muted">Has not run yet</span>
                }
              </ng-template>
              <ng-template pdCell="status" let-r><span class="st" [attr.data-kind]="kindOf(r)" [attr.title]="why(r) ?? status(r)"><dy-icon [name]="kindOf(r) === 'armed' ? 'check' : kindOf(r) === 'off' || kindOf(r) === 'paused' ? 'pause' : 'alert'" /><span class="txt">{{ status(r) }}</span></span></ng-template>
            </pd-table>
            @for (r of failed(); track r.entry.id) {
              <p class="detail"><dy-icon name="alert" /><span>{{ r.entry.name }}: not set. {{ why(r) }}</span><button type="button" matButton="outlined" [disabled]="store.busy()" (click)="retry(r)">Retry</button></p>
            }
          }
        </section>

        @if (store.actionError(); as e) {
          @if (!form() || !fieldError()) {
            <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></div>
          }
        }

        @if (form(); as f) {
          <section class="form" aria-label="Scheduled wake">
            <h2>{{ f.id === null ? 'Add a schedule' : 'Change a schedule' }}</h2>
            <div class="fields">
              <mat-form-field><mat-label>Name</mat-label><input matInput [ngModel]="f.name" (ngModelChange)="patch({ name: $event })" aria-label="Name of the schedule" maxlength="48" [errorStateMatcher]="matchers.name" />@if (fieldError() === 'name') { <mat-error>{{ store.actionError()?.message }}</mat-error> }</mat-form-field>
              <mat-form-field><mat-label>Wake at</mat-label><input matInput inputmode="numeric" autocomplete="off" [placeholder]="example" [ngModel]="time()" (ngModelChange)="setTime($event)" aria-label="Time of day to wake" [errorStateMatcher]="matchers.time" />@if (fieldError() === 'time') { <mat-error>{{ store.actionError()?.message }}</mat-error> }</mat-form-field>
            </div>
            <fieldset class="rep" [attr.aria-invalid]="fieldError() === 'repeat' ? 'true' : null">
              <legend>Repeat</legend>
              <mat-button-toggle-group class="chips" aria-label="Repeat" [value]="repeatKind()" (change)="pickRepeat($event.value)">
                <mat-button-toggle value="once">Once</mat-button-toggle>
                <mat-button-toggle value="daily">Every day</mat-button-toggle>
                <mat-button-toggle value="days">On chosen days</mat-button-toggle>
              </mat-button-toggle-group>
              @if (repeatKind() === 'days') {
                <div class="days" role="group" aria-label="Days of the week">
                  @for (d of weekdays; track d; let i = $index) {
                    <mat-checkbox [checked]="dayOn(i)" (change)="toggleDay(i, $event.checked)">{{ d }}</mat-checkbox>
                  }
                  <button type="button" matButton="outlined" (click)="setRepeat({ days: { mask: 31 } })">Weekdays</button>
                </div>
              }
              @if (fieldError() === 'repeat') {
                <p class="group-error" role="alert"><dy-icon name="alert" /><span>{{ store.actionError()?.message }}</span></p>
              }
            </fieldset>
            <fieldset class="then" [attr.aria-invalid]="fieldError() === 'then' ? 'true' : null">
              <legend>After it wakes</legend>
              <div class="row">
                <mat-checkbox [checked]="f.stayAwakeMinutes !== null" (change)="patch({ stayAwakeMinutes: $event.checked ? 30 : null })">Stay awake for</mat-checkbox>
                <mat-form-field class="num"><input matInput type="number" min="1" max="720" [disabled]="f.stayAwakeMinutes === null" [ngModel]="f.stayAwakeMinutes ?? 30" (ngModelChange)="patch({ stayAwakeMinutes: +$event })" aria-label="Minutes to stay awake" /><span matTextSuffix>minutes</span></mat-form-field>
              </div>
              <div class="grp" role="group" aria-label="Wake these computers">
                <span class="sub">Wake these computers</span>
                <div class="devs">
                  @for (d of devices(); track d.id) {
                    <mat-checkbox [checked]="f.wakeDevices.includes(d.id)" (change)="toggleDevice(d.id, $event.checked)">{{ d.name }}</mat-checkbox>
                  } @empty {
                    <span class="muted">No computers saved for Wake-on-LAN yet (Wake devices tab).</span>
                  }
                </div>
              </div>
              <div class="grp" role="group" aria-label="Run a Dyno job">
                <span class="sub">Run a Dyno job</span>
                <div class="jobs">
                  @for (j of jobs; track j.id) {
                    <mat-checkbox [checked]="f.job === j.id" (change)="patch({ job: $event.checked ? j.id : null })">{{ j.label }}</mat-checkbox>
                  }
                </div>
              </div>
              @if (fieldError() === 'then') {
                <p class="group-error" role="alert"><dy-icon name="alert" /><span>{{ store.actionError()?.message }}</span></p>
              }
            </fieldset>
            <mat-slide-toggle [checked]="f.enabled" (change)="patch({ enabled: $event.checked })" aria-label="Switched on">Switched on</mat-slide-toggle>
            <div class="acts">
              <button type="button" matButton="filled" [disabled]="store.busy()" (click)="save()">{{ f.id === null ? 'Add schedule' : 'Save changes' }}</button>
              <button type="button" matButton="text" [disabled]="store.busy()" (click)="closeForm()">Cancel</button>
            </div>
          </section>
        }
      }
    </dy-async>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-4); }
    dy-async { display: grid; gap: var(--sp-4); }
    section { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-4); display: grid; gap: var(--sp-3); }
    h2 { margin: 0; font-size: 18px; font-weight: var(--fw-semibold); }
    .head { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); flex-wrap: wrap; }
    .acts { display: flex; gap: var(--sp-2); flex-wrap: wrap; align-items: center; }
    .muted { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .note dy-icon { flex: none; margin-top: 2px; }
    .note { display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); flex-wrap: wrap; }
    .note > span { flex: 1 1 24ch; min-width: 0; }
    .note.warn { color: var(--warn); }
    .note.warn span { color: var(--text-1); }
    .note.bad, .group-error { color: var(--bad); }
    .group-error { display: flex; gap: var(--sp-1); align-items: center; margin: 0; font-size: var(--fs-sm); }
    .cant { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); color: var(--text-2); }
    .cant li { display: flex; gap: var(--sp-2); align-items: flex-start; }
    .cant dy-icon { --icon-size: 14px; flex: none; margin-top: 2px; color: var(--text-3); }
    .detail { display: flex; gap: var(--sp-2); align-items: center; margin: 0; font-size: var(--fs-sm); color: var(--text-1); flex-wrap: wrap; }
    .detail dy-icon { color: var(--warn); --icon-size: 14px; }
    .empty { display: grid; justify-items: center; gap: var(--sp-1); padding: var(--sp-6) var(--sp-4); text-align: center; color: var(--text-2); }
    .empty dy-icon { --icon-size: 28px; color: var(--text-3); }
    .empty strong { color: var(--text-1); font-weight: var(--fw-medium); }
    .empty span { font-size: var(--fs-sm); max-width: 52ch; }
    .nm { font-weight: var(--fw-medium); display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .txt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .wrap { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; white-space: normal; line-height: 1.25; min-width: 0; }
    .st { display: inline-flex; gap: var(--sp-1); align-items: center; min-width: 0; max-width: 100%; font-size: var(--fs-sm); color: var(--text-2); }
    .st[data-kind='armed'] dy-icon { color: var(--ok); }
    .st[data-kind='needsHelper'] dy-icon, .st[data-kind='failed'] dy-icon { color: var(--warn); }
    .st dy-icon { --icon-size: 14px; flex: none; }
    .fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--sp-3); align-items: start; }
    .fields mat-form-field { width: 100%; }
    fieldset { border: 0; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    fieldset[aria-invalid='true'] { outline: 1px solid var(--bad); outline-offset: 6px; border-radius: var(--r-md); }
    legend { padding: 0; font-size: var(--fs-sm); color: var(--text-2); margin-bottom: var(--sp-1); }
    .sub { font-size: var(--fs-sm); color: var(--text-2); }
    .grp { display: grid; gap: var(--sp-1); }
    .chips, .days, .devs, .jobs { display: flex; gap: var(--sp-2); flex-wrap: wrap; align-items: center; }
    .row { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
    .num { width: 170px; }
    @media (max-width: 760px) { .num { width: 100%; } }
  `,
})
export class ScheduleTab implements OnInit {
  protected readonly store = inject(WakesStore);
  protected readonly helper = inject(HelperStore);
  protected readonly settings = inject(SettingsStore);
  private readonly wol = inject(WolStore);
  private readonly dialogs = inject(DialogService);
  private readonly host = inject(ElementRef<HTMLElement>);

  protected readonly weekdays = WEEKDAYS;
  protected readonly jobs: { id: DynoJob; label: string }[] = (Object.keys(JOB_LABELS) as DynoJob[]).map((id) => ({ id, label: JOB_LABELS[id] }));
  protected readonly form = signal<WakeInput | null>(null);
  protected readonly time = signal('07:00');
  protected readonly devices = computed(() => this.wol.view()?.devices ?? []);
  protected readonly wakeEnabled = computed(() => this.settings.settings()?.power.wake.enabled ?? false);
  protected readonly fieldError = computed<Field | null>(() => (this.form() ? wakeFieldOf(this.store.actionError()?.message) : null));
  protected readonly repeatKind = computed(() => {
    const r = this.form()?.repeat;
    return r === undefined ? 'daily' : typeof r === 'string' ? r : 'days';
  });
  /** Nothing can set a wake here: Add schedule is disabled with the reason. */
  protected readonly unavailable = computed(() => {
    const r = this.store.view()?.route;
    return r && typeof r === 'object' && 'unavailable' in r ? r.unavailable.reason : null;
  });
  protected readonly addBlocked = computed(() => !this.wakeEnabled() || this.unavailable() !== null);
  protected readonly addReason = computed(() => (!this.wakeEnabled() ? 'turn on scheduled wake first.' : this.unavailable()));
  /** The page's one primary button is Add schedule only while it can be used and nothing else is asking for the person's attention. */
  protected readonly addPrimary = computed(() => !this.addBlocked() && routeKind(this.store.view()?.route ?? 'helperReady') !== 'needsHelper');
  /** Rows whose wake the OS refused and that a retry could fix (not where nothing can ever set a wake). */
  protected readonly failed = computed(() => (this.unavailable() !== null ? [] : (this.store.view()?.rows ?? []).filter((r) => failedWhy(r) !== null)));
  private readonly matcher = (field: Field): ErrorStateMatcher => ({ isErrorState: () => this.fieldError() === field });
  protected readonly matchers = { name: this.matcher('name'), time: this.matcher('time') };

  protected readonly blank = blankWake;
  protected readonly clock = clock;
  protected readonly repeatText = repeatText;
  protected readonly routeKind = routeKind;
  protected readonly routeText = routeText;
  protected readonly cantTell = routeCantTell;
  protected readonly rowKey = (r: WakeRow): string => `${r.entry.id}`;
  protected readonly kindOf = (r: WakeRow) => rowKind(r.state);
  protected readonly status = statusText;
  protected readonly why = failedWhy;
  protected readonly last = (r: WakeRow) => lastRunShort(r.entry);
  protected readonly then = (r: WakeRow) => thenText(r.entry, (id) => this.devices().find((d) => d.id === id)?.name ?? 'a computer');
  protected readonly columns: PdColumn<WakeRow>[] = [
    { key: 'name', header: 'Name', value: (r) => r.entry.name, required: true, width: 'minmax(0, 230px)' },
    { key: 'when', header: 'When', value: (r) => r.entry.minuteOfDay, width: '130px', hideBelow: 640 },
    { key: 'then', header: 'Then', value: (r) => r.entry.stayAwakeMinutes ?? 0, width: 'minmax(0, 1.2fr)', hideBelow: 760 },
    { key: 'last', header: 'Last run', value: (r) => r.entry.lastRun?.at ?? 0, width: 'minmax(0, 1.3fr)', hideBelow: 900 },
    { key: 'status', header: 'Status', value: (r) => statusText(r), width: 'minmax(0, 1fr)' },
  ];
  protected readonly actions = (r?: WakeRow): PdRowAction[] => [
    { id: 'edit', label: 'Change', icon: 'edit', inline: true },
    ...(r && failedWhy(r) !== null && this.unavailable() === null ? [{ id: 'retry', label: 'Retry', icon: 'refresh' }] : []),
    { id: 'cancel', label: 'Cancel schedule', icon: 'trash', danger: true },
  ];

  ngOnInit(): void {
    void this.store.load();
    void this.wol.load();
    void this.helper.ensure();
    if (!this.settings.settings()) void this.settings.load();
  }

  protected run({ id, row }: PdRowActionEvent<WakeRow>): void {
    if (id === 'edit') this.edit(row);
    else if (id === 'retry') void this.retry(row);
    else if (id === 'cancel') void this.remove(row);
  }

  protected edit(r: WakeRow): void {
    this.openForm(wakeInputFrom(r.entry));
  }

  /** Saves the schedule as it is, which sets its OS wake again. */
  protected async retry(r: WakeRow): Promise<void> {
    await this.store.save(wakeInputFrom(r.entry));
  }

  /** Turns scheduled wake on in the settings (no trip to the Settings window), then reads the schedules again. */
  protected async turnOnWake(): Promise<void> {
    const s = this.settings.settings();
    if (!s) return;
    if (await this.settings.save({ ...s, power: { ...s.power, wake: { ...s.power.wake, enabled: true } } }, 'Scheduled wake is on')) await this.store.load();
  }

  /** Turns the administrator helper on (the OS's one prompt), then reads the schedules again. */
  protected async turnOnHelper(): Promise<void> {
    await this.helper.start('turnOn');
    await this.store.load();
  }

  protected openForm(f: WakeInput): void {
    this.store.clearActionError();
    this.time.set(clock(f.minuteOfDay));
    this.form.set(f);
  }

  protected closeForm(): void {
    this.form.set(null);
    this.store.clearActionError();
  }

  protected patch(p: Partial<WakeInput>): void {
    this.form.update((f) => (f ? { ...f, ...p } : f));
  }

  protected setTime(text: string): void {
    this.time.set(text);
    const m = minuteOf(text);
    // A time the control cannot make into a time of day goes to the agent as it is, out of range, so its message answers.
    this.patch({ minuteOfDay: m ?? 24 * 60 });
    // (a text the box cannot read is out of range on purpose)
  }

  /** What the time box expects, in the system's own format (24-hour or 12-hour). */
  protected readonly example = `e.g. ${clock(7 * 60 + 30)}`;

  protected pickRepeat(kind: 'once' | 'daily' | 'days'): void {
    if (kind === 'days') this.chooseDays();
    else this.setRepeat(kind);
  }

  protected setRepeat(r: WakeRepeat): void {
    this.patch({ repeat: r });
  }

  protected chooseDays(): void {
    if (this.repeatKind() !== 'days') this.patch({ repeat: { days: { mask: 0 } } });
  }

  protected dayOn(i: number): boolean {
    const r = this.form()?.repeat;
    return typeof r === 'object' && (r.days.mask & (1 << i)) !== 0;
  }

  protected toggleDay(i: number, on: boolean): void {
    const r = this.form()?.repeat;
    const mask = typeof r === 'object' ? r.days.mask : 0;
    this.patch({ repeat: { days: { mask: on ? mask | (1 << i) : mask & ~(1 << i) } } });
  }

  protected toggleDevice(id: number, on: boolean): void {
    const cur = this.form()?.wakeDevices ?? [];
    this.patch({ wakeDevices: on ? [...cur, id] : cur.filter((d) => d !== id) });
  }

  protected async save(): Promise<void> {
    const f = this.form();
    if (!f) return;
    if (await this.store.save(f)) this.closeForm();
    else this.focusError();
  }

  /** Puts the focus on the field or group the engine's message is about (after the view shows it). */
  private focusError(): void {
    const field = this.fieldError();
    const selector =
      field === 'time'
        ? 'input[aria-label="Time of day to wake"]'
        : field === 'then'
          ? 'fieldset.then mat-checkbox input'
          : field === 'repeat'
            ? 'fieldset.rep button'
            : 'input[aria-label="Name of the schedule"]';
    queueMicrotask(() => (this.host.nativeElement as HTMLElement).querySelector<HTMLElement>(selector)?.focus());
  }

  protected async remove(r: WakeRow): Promise<void> {
    const yes = await confirm(this.dialogs, {
      heading: `Cancel “${r.entry.name}”?`,
      message: 'Dyno forgets this schedule and clears its wake from the computer.',
      confirm: { label: 'Cancel schedule', destructive: true },
    });
    if (yes) await this.store.cancel(r.entry.id);
  }
}
