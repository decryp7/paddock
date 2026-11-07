import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButton } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { IdleAction, IdleSettings, PowerSettings, TimedAction } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { injectNow } from '../../shared/now';
import { PowerStore } from '../../state/power.store';
import { SettingsStore } from '../../state/settings.store';
import { DEFAULT_POWER, actionVerb, actionWarning, cantTellReason, clockOf, idleHeadline, idleKind, idleRuleLine, minuteOfDay, repeatText } from './power-model';

const ACTIONS: readonly { id: IdleAction; label: string }[] = [
  { id: 'sleep', label: 'Sleep' },
  { id: 'hibernate', label: 'Hibernate' },
  { id: 'shutDown', label: 'Shut down' },
];
const REPEATS = [
  { id: 'once', label: 'Once', value: 'once' as const },
  { id: 'daily', label: 'Every day', value: 'daily' as const },
  { id: 'weekdays', label: 'Monday to Friday', value: { weekdays: 0b0011111 } },
  { id: 'weekend', label: 'Saturday and Sunday', value: { weekdays: 0b1100000 } },
] as const;

/**
 * Sleep and shut down: after the computer has been idle, or at a time of day. Off until you turn it on; a countdown with Cancel comes
 * first; Dyno asks the system the normal way and never forces it (an app with unsaved work may stop a shutdown). Anything Dyno cannot
 * read blocks the action and is listed here with its reason.
 */
@Component({
  selector: 'dy-sleep-tab',
  imports: [AsyncState, Icon, FormsModule, MatButton, MatFormFieldModule, MatInputModule, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!store.view() && !!draft()" [error]="store.error()" what="the sleep and shut down rules" [rows]="3" (retry)="store.load()">
      @if (store.view(); as v) {
        <section class="status" aria-label="Sleep and shut down status" aria-live="polite">
          <h2 [attr.data-kind]="kind()">{{ headline() }}</h2>
          @switch (kind()) {
            @case ('countdown') {
              <p>Nothing happens until it reaches zero, and Dyno reads everything once more first. Moving the mouse cancels an idle countdown.</p>
              <div class="acts">
                <button type="button" matButton="filled" [disabled]="store.busy()" (click)="store.idleCancel()">Cancel {{ verb() }}</button>
                <button type="button" matButton="outlined" [disabled]="store.busy()" (click)="store.idleSnooze(30)">Snooze 30 minutes</button>
              </div>
            }
            @case ('acting') {
              <p>Dyno asked the system the normal way. If an app asks you to save, answer it there; Dyno waits and does not try again.</p>
              <button class="fit" type="button" matButton="outlined" [disabled]="store.busy()" (click)="store.idleCancel()">Stop waiting</button>
            }
            @case ('stopped') {
              <p>Dyno will not try again until the computer is used.</p>
            }
            @case ('unavailable') {
              <p>The timed actions below still work.</p>
            }
            @case ('watching') {
              @if (blockers().length > 0) {
                <ul class="blockers" aria-label="What is in the way">
                  @for (b of blockers(); track b.reason) {
                    <li>{{ cap(b.reason) }} <strong>({{ b.cantTell ? "can't tell" : 'waiting' }})</strong></li>
                  }
                </ul>
                <p class="hint">Anything Dyno cannot read counts as busy. You can switch an exception off below.</p>
              } @else if (!snoozed()) {
                <button class="fit" type="button" matButton="outlined" [disabled]="store.busy()" (click)="store.idleCancel()">Cancel for this idle period</button>
              }
            }
            @default {
              <p>Dyno never sleeps or shuts down your computer unless you turn a rule on here.</p>
            }
          }
        </section>

        @if (store.actionError(); as e) {
          <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></div>
        }

        <section class="rule" aria-label="When the computer is idle">
          <h2>When the computer is idle</h2>
          <div class="row">
            <div class="lbl"><strong>Act when idle</strong><small>{{ warning(draft()!.idle.action) }}</small></div>
            <mat-slide-toggle [checked]="draft()!.idle.enabled" aria-label="Act when the computer is idle" (change)="setIdle({ enabled: $event.checked })" />
          </div>
          <div class="row" [class.off]="!draft()!.idle.enabled">
            <div class="lbl"><strong>Do</strong></div>
            <select aria-label="What to do when idle" [ngModel]="draft()!.idle.action" (ngModelChange)="setIdle({ action: $event })">
              @for (a of actions; track a.id) {
                <option [value]="a.id" [disabled]="!!why(a.id)">{{ a.label }}{{ why(a.id) ? ' (not available: ' + why(a.id) + ')' : '' }}</option>
              }
            </select>
          </div>
          <div class="row" [class.off]="!draft()!.idle.enabled">
            <div class="lbl"><strong>After</strong><small>How long with no keyboard or mouse (1 to 1440 minutes).</small></div>
            <mat-form-field class="num-in"><input matInput type="number" min="1" max="1440" aria-label="Minutes idle before acting" [ngModel]="draft()!.idle.afterSecs / 60" (ngModelChange)="setIdle({ afterSecs: +$event * 60 })" /><span matTextSuffix>min</span></mat-form-field>
          </div>
          <div class="row" [class.off]="!draft()!.idle.enabled">
            <div class="lbl"><strong>Warn me for</strong><small>The countdown before it acts (30 to 600 seconds).</small></div>
            <mat-form-field class="num-in"><input matInput type="number" min="30" max="600" aria-label="Seconds of warning" [ngModel]="draft()!.idle.countdownSecs" (ngModelChange)="setIdle({ countdownSecs: +$event })" /><span matTextSuffix>s</span></mat-form-field>
          </div>
          <div class="row" [class.off]="!draft()!.idle.enabled">
            <div class="lbl"><strong>Only when plugged in</strong><small>Checked again when the countdown ends. A laptop on battery is left alone.</small></div>
            <mat-slide-toggle [checked]="draft()!.idle.acOnly" aria-label="Only when plugged in" (change)="setIdle({ acOnly: $event.checked })" />
          </div>
          <fieldset class="exceptions" [class.off]="!draft()!.idle.enabled">
            <legend>Not while</legend>
            <div class="row"><div class="lbl"><strong>Audio is playing</strong></div><mat-slide-toggle [checked]="draft()!.idle.exceptions.audio" aria-label="Not while audio is playing" (change)="setEx({ audio: $event.checked })" /></div>
            <div class="row"><div class="lbl"><strong>An app is full screen or presenting</strong></div><mat-slide-toggle [checked]="draft()!.idle.exceptions.fullScreen" aria-label="Not while an app is full screen" (change)="setEx({ fullScreen: $event.checked })" /></div>
            <div class="row"><div class="lbl"><strong>Another program asked the computer to stay awake</strong><small>It may be running or have unsaved work.</small></div><mat-slide-toggle [checked]="draft()!.idle.exceptions.otherStayAwake" aria-label="Not while another program asked the computer to stay awake" (change)="setEx({ otherStayAwake: $event.checked })" /></div>
            <div class="row"><div class="lbl"><strong>The CPU is above</strong></div><mat-form-field class="num-in"><input matInput type="number" min="1" max="100" aria-label="CPU percent" [ngModel]="draft()!.idle.exceptions.cpuPct" (ngModelChange)="setEx({ cpuPct: num($event) })" /><span matTextSuffix>%</span></mat-form-field></div>
            <div class="row"><div class="lbl"><strong>The disk is above</strong></div><mat-form-field class="num-in"><input matInput type="number" min="1" aria-label="Disk megabytes per second" [ngModel]="mb(draft()!.idle.exceptions.diskBytesPerS)" (ngModelChange)="setEx({ diskBytesPerS: bytes($event, 1048576) })" /><span matTextSuffix>MB/s</span></mat-form-field></div>
            <div class="row"><div class="lbl"><strong>The network is above</strong></div><mat-form-field class="num-in"><input matInput type="number" min="1" aria-label="Network kilobytes per second" [ngModel]="kb(draft()!.idle.exceptions.netBytesPerS)" (ngModelChange)="setEx({ netBytesPerS: bytes($event, 1024) })" /><span matTextSuffix>KB/s</span></mat-form-field></div>
            <div class="row top"><div class="lbl"><strong>One of these programs is running</strong><small>File names, one per line.</small></div><mat-form-field><textarea matInput rows="2" spellcheck="false" aria-label="Programs that block" [ngModel]="programs()" (ngModelChange)="setEx({ programs: lines($event) })"></textarea></mat-form-field></div>
          </fieldset>
        </section>

        <section class="rule" aria-label="At a time of day">
          <h2>At a time of day</h2>
          <div class="row">
            <div class="lbl"><strong>Run timed actions</strong><small>The same warning and Cancel come first. Nothing wakes the computer from shut down.</small></div>
            <mat-slide-toggle [checked]="draft()!.timed.enabled" aria-label="Run timed actions" (change)="setTimed({ enabled: $event.checked })" />
          </div>
          @if (draft()!.timed.entries.length === 0) {
            <p class="hint">No timed actions yet.</p>
          } @else {
            <table aria-label="Timed actions">
              <thead><tr><th scope="col">Time</th><th scope="col">Action</th><th scope="col">Repeats</th><th scope="col"><span class="sr">Remove</span></th></tr></thead>
              <tbody>
                @for (t of draft()!.timed.entries; track t.id) {
                  <tr>
                    <th scope="row">{{ clock(t.atMinute) }}</th>
                    <td>{{ verbOf(t.action) }}</td>
                    <td>{{ repeat(t) }}{{ t.done ? ' (done)' : '' }}</td>
                    <td><button type="button" matButton="text" [attr.aria-label]="'Remove ' + verbOf(t.action) + ' at ' + clock(t.atMinute)" (click)="remove(t.id)">Remove</button></td>
                  </tr>
                }
              </tbody>
            </table>
          }
          <div class="row add">
            <select aria-label="Timed action" [ngModel]="newAction()" (ngModelChange)="newAction.set($event)">
              @for (a of actions; track a.id) {
                <option [value]="a.id" [disabled]="!!why(a.id)">{{ a.label }}</option>
              }
            </select>
            <mat-form-field class="time-in"><mat-label>At</mat-label><input matInput type="time" aria-label="Time of day" [ngModel]="newTime()" (ngModelChange)="newTime.set($event)" /></mat-form-field>
            <select aria-label="How often" [ngModel]="newRepeat()" (ngModelChange)="newRepeat.set($event)">
              @for (r of repeats; track r.id) {
                <option [value]="r.id">{{ r.label }}</option>
              }
            </select>
            <button type="button" matButton="outlined" [disabled]="minute() === null || draft()!.timed.entries.length >= 16" (click)="add()">Add</button>
          </div>
        </section>

        <div class="save">
          <button type="button" matButton="filled" [disabled]="settings.saving() || !dirty()" (click)="save()">Save changes</button>
          @if (settings.saveError(); as e) {
            <span class="problem" role="alert">{{ e.message }}</span>
          }
        </div>
        <p class="foot">{{ footer(v.caps.sleep) }}</p>
      }
    </dy-async>
  `,
  styles: `
    :host { display: block; padding-top: var(--sp-5); }
    dy-async { display: grid; gap: var(--sp-4); }
    section { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-4); display: grid; gap: var(--sp-3); }
    h2 { margin: 0; font-size: 18px; font-weight: var(--fw-semibold); }
    .status h2[data-kind='countdown'] { color: var(--warn); }
    .status p, .hint, .foot { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .hint, .foot { color: var(--text-3); }
    .acts { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
    .blockers { margin: 0; padding-left: var(--sp-4); font-size: var(--fs-sm); color: var(--text-2); }
    .note { display: flex; gap: var(--sp-2); align-items: center; font-size: var(--fs-sm); }
    .note.bad, .problem { color: var(--bad); }
    .row { display: flex; gap: var(--sp-3); align-items: center; justify-content: space-between; flex-wrap: wrap; }
    .row.top { align-items: flex-start; }
    .row.add { justify-content: flex-start; }
    .row.off, .exceptions.off { opacity: 0.55; }
    .lbl { display: grid; gap: 2px; max-width: 60ch; }
    .lbl small { color: var(--text-3); font-size: var(--fs-sm); }
    .exceptions { border: 0; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    .exceptions legend { padding: 0; font-weight: var(--fw-semibold); margin-bottom: var(--sp-2); }
    .time-in { width: 140px; }
    .num-in { width: 150px; }
    .fit { justify-self: start; }
    select { font: inherit; color: inherit; background: var(--surface-1, transparent); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 6px 8px; }
    table { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); }
    th, td { text-align: left; padding: 6px var(--sp-3); border-bottom: 1px solid var(--border); }
    thead th { color: var(--text-3); font-weight: var(--fw-medium); }
    .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
    .save { display: flex; gap: var(--sp-3); align-items: center; }
  `,
})
export class SleepTab implements OnInit {
  protected readonly store = inject(PowerStore);
  protected readonly settings = inject(SettingsStore);
  private readonly now = injectNow(1000);

  protected readonly actions = ACTIONS;
  protected readonly repeats = REPEATS;
  protected readonly draft = signal<PowerSettings | null>(null);
  protected readonly newAction = signal<IdleAction>('sleep');
  protected readonly newTime = signal('');
  protected readonly newRepeat = signal<(typeof REPEATS)[number]['id']>('daily');

  protected readonly kind = computed(() => (this.store.view() ? idleKind(this.store.view()!.idle) : 'disabled'));
  protected readonly headline = computed(() => {
    const v = this.store.view()?.idle;
    if (!v) return '';
    const rule = this.settings.settings()?.power.idle;
    // While watching with nothing in the way, the card says what the rule is and how long is left.
    if (typeof v === 'object' && 'watching' in v && rule?.enabled && v.watching.blockers.length === 0 && v.watching.snoozedUntil === null) {
      return idleRuleLine(rule.action, rule.afterSecs, v.watching.idleSecs);
    }
    return idleHeadline(v, this.now());
  });
  protected readonly snoozed = computed(() => {
    const v = this.store.view()?.idle;
    return !!v && typeof v === 'object' && 'watching' in v && v.watching.snoozedUntil !== null;
  });
  protected readonly blockers = computed(() => {
    const v = this.store.view()?.idle;
    return v && typeof v === 'object' && 'watching' in v ? v.watching.blockers : [];
  });
  protected readonly verb = computed(() => {
    const v = this.store.view()?.idle;
    return v && typeof v === 'object' && 'countdown' in v ? actionVerb(v.countdown.action) : 'it';
  });
  protected readonly minute = computed(() => minuteOfDay(this.newTime()));
  protected readonly programs = computed(() => (this.draft()?.idle.exceptions.programs ?? []).join('\n'));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.settings.settings()?.power ?? null));

  protected cap(text: string): string {
    return text.charAt(0).toUpperCase() + text.slice(1);
  }

  protected readonly clock = clockOf;
  protected readonly repeat = (t: TimedAction): string => repeatText(t.repeat);
  protected readonly warning = actionWarning;
  protected readonly verbOf = actionVerb;

  ngOnInit(): void {
    void this.store.load();
    const set = (s: PowerSettings | undefined): void => this.draft.set(structuredClone(s ?? DEFAULT_POWER));
    if (this.settings.settings()) set(this.settings.settings()!.power);
    else void this.settings.load().then(() => set(this.settings.settings()?.power));
  }

  /** Why an action cannot be asked for here, or null. */
  protected why(a: IdleAction): string | null {
    const c = this.store.view()?.caps;
    if (!c) return null;
    return cantTellReason(a === 'sleep' ? c.sleep : a === 'hibernate' ? c.hibernate : c.shutDown);
  }

  protected footer(sleep: unknown): string {
    return sleep === 'known' ? 'Dyno always asks the system the normal way and never forces it.' : 'Dyno cannot ask this system to sleep; the rules below will say why.';
  }

  protected setIdle(p: Partial<IdleSettings>): void {
    this.draft.update((d) => (d ? { ...d, idle: { ...d.idle, ...p } } : d));
  }
  protected setEx(p: Partial<IdleSettings['exceptions']>): void {
    this.draft.update((d) => (d ? { ...d, idle: { ...d.idle, exceptions: { ...d.idle.exceptions, ...p } } } : d));
  }
  protected setTimed(p: Partial<PowerSettings['timed']>): void {
    this.draft.update((d) => (d ? { ...d, timed: { ...d.timed, ...p } } : d));
  }
  protected num(v: unknown): number | null {
    return v === null || v === undefined || v === '' || Number.isNaN(Number(v)) ? null : Number(v);
  }
  protected bytes(v: unknown, unit: number): number | null {
    const n = this.num(v);
    return n === null ? null : Math.round(n * unit);
  }
  protected mb(b: number | null): number | null {
    return b === null ? null : Math.round(b / 1048576);
  }
  protected kb(b: number | null): number | null {
    return b === null ? null : Math.round(b / 1024);
  }
  protected lines(text: string): string[] {
    return text.split('\n').map((l) => l.trim()).filter((l) => l.length > 0);
  }

  protected add(): void {
    const m = this.minute();
    if (m === null) return;
    const repeat = REPEATS.find((r) => r.id === this.newRepeat())!.value;
    this.draft.update((d) => {
      if (!d) return d;
      const id = d.timed.entries.reduce((a, e) => Math.max(a, e.id), 0) + 1;
      return { ...d, timed: { ...d.timed, entries: [...d.timed.entries, { id, action: this.newAction(), atMinute: m, repeat, lastDay: null, done: false }] } };
    });
    this.newTime.set('');
  }
  protected remove(id: number): void {
    this.draft.update((d) => (d ? { ...d, timed: { ...d.timed, entries: d.timed.entries.filter((e) => e.id !== id) } } : d));
  }

  protected async save(): Promise<void> {
    const cur = this.settings.settings();
    const d = this.draft();
    if (!cur || !d) return;
    if (await this.settings.save({ ...cur, power: d }, 'Sleep and shut down rules saved')) this.draft.set(structuredClone(this.settings.settings()!.power));
  }
}
