import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { CommandError, CustomAlertRule, CustomMetric, CustomOp, Severity } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { Switch } from '../../shared/switch';
import { CUSTOM_METRICS, metricInfo, severityWord, uniqueRuleId } from './alarms-model';

export interface RuleDialogData {
  /** The rule to edit, or null for a new one. */
  rule: CustomAlertRule | null;
  /** Ids of the other rules (a new id must avoid them). */
  takenIds: string[];
  /** Saves the rule; answers the engine's error to show inline, or null once stored. */
  save: (rule: CustomAlertRule) => Promise<CommandError | null>;
}

const SEVERITIES: Severity[] = ['info', 'warning', 'critical'];
const MAX_HOLD_SECS = 3600;

/** Add or edit a custom alert rule: a metric, above or below a limit, held for a while, with a severity. */
@Component({
  selector: 'dy-rule-dialog',
  imports: [DialogFrame, Icon, Switch, CdkMenu, CdkMenuItem, CdkMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="data.rule ? 'Edit alert rule' : 'Add alert rule'" subtitle="Raises an alarm when a reading stays past a limit." icon="bell">
      <div class="field">
        <label for="rule-name">Name</label>
        <input id="rule-name" class="input" type="text" maxlength="60" placeholder="High CPU while idle" [value]="name()" (input)="name.set($any($event.target).value)" />
      </div>
      <div class="field">
        <span class="label" id="rule-metric-l">Reading</span>
        <button type="button" class="input pick" aria-labelledby="rule-metric-l" [cdkMenuTriggerFor]="metricMenu"><span>{{ info().label }}</span><span class="faint">{{ info().unit }}</span><dy-icon name="chevron-down" /></button>
      </div>
      <div class="two">
        <div class="field">
          <span class="label" id="rule-op-l">Alarm when it is</span>
          <div class="seg" role="radiogroup" aria-labelledby="rule-op-l">
            @for (o of ops; track o) {
              <button type="button" role="radio" [attr.aria-checked]="op() === o" (click)="op.set(o)">{{ o === 'above' ? 'Above' : 'Below' }}</button>
            }
          </div>
        </div>
        <div class="field">
          <span class="label" id="rule-limit-l">Limit</span>
          <div class="stepper" role="group" aria-labelledby="rule-limit-l">
            <button type="button" class="ib" aria-label="Decrease the limit" (click)="step(-1)"><dy-icon name="minus" /></button>
            <span class="in"><input class="input" type="text" inputmode="decimal" aria-label="Limit" [value]="limit()" (input)="limit.set($any($event.target).value)" /><span class="unit">{{ info().unit }}</span></span>
            <button type="button" class="ib" aria-label="Increase the limit" (click)="step(1)"><dy-icon name="plus" /></button>
          </div>
        </div>
      </div>
      <div class="two">
        <div class="field">
          <span class="label" id="rule-for-l">For at least</span>
          <div class="hold" role="group" aria-labelledby="rule-for-l">
            <input class="input" type="text" inputmode="numeric" aria-label="Duration" [value]="hold()" (input)="hold.set($any($event.target).value)" />
            <div class="seg" role="radiogroup" aria-label="Duration unit">
              <button type="button" role="radio" [attr.aria-checked]="unit() === 'sec'" (click)="setUnit('sec')">sec</button>
              <button type="button" role="radio" [attr.aria-checked]="unit() === 'min'" (click)="setUnit('min')">min</button>
            </div>
          </div>
          <span class="hint">0 raises the alarm right away. At most 60 min.</span>
        </div>
        <div class="field">
          <span class="label" id="rule-sev-l">Severity</span>
          <div class="seg" role="radiogroup" aria-labelledby="rule-sev-l">
            @for (s of severities; track s) {
              <button type="button" role="radio" [attr.aria-checked]="severity() === s" (click)="severity.set(s)">{{ word(s) }}</button>
            }
          </div>
        </div>
      </div>
      <div class="on"><span>Enabled</span><dy-switch [checked]="enabled()" (changed)="enabled.set($event)" label="Enabled" /></div>
      @if (problem(); as p) {
        <p class="err" role="alert"><dy-icon name="alert" /> {{ p }}</p>
      }
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="busy()" (click)="submit()">{{ data.rule ? 'Save' : 'Add rule' }}</button>
      </ng-container>
    </dy-dialog>

    <ng-template #metricMenu>
      <div class="menu wide" cdkMenu aria-label="Reading">
        @for (m of metrics; track m.id) {
          <button type="button" class="menu-item two-line" cdkMenuItem [attr.aria-checked]="metric() === m.id" (cdkMenuItemTriggered)="metric.set(m.id)">
            <span class="text"><span>{{ m.label }} <span class="faint">({{ m.unit }})</span></span><small>{{ m.hint }}</small></span>
          </button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { --dialog-w: 480px; display: contents; }
    .two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-4); align-items: start; }
    .pick { display: flex; align-items: center; gap: var(--sp-2); width: 100%; text-align: left; cursor: pointer; }
    .pick span:first-child { flex: 1; }
    .pick dy-icon { --icon-size: 14px; color: var(--text-3); }
    .faint { color: var(--text-3); }
    .hint { color: var(--text-3); font-size: var(--fs-xs); }
    .stepper { display: flex; align-items: center; gap: 2px; }
    .ib { display: grid; place-items: center; width: 28px; height: var(--control-h); flex: none; border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-2); color: var(--text-2); cursor: pointer; }
    .ib:hover { background: var(--surface-hover); color: var(--text-1); }
    .ib dy-icon { --icon-size: 14px; }
    .in { position: relative; flex: 1; min-width: 0; }
    .in input { width: 100%; padding-right: 30px; text-align: right; font-variant-numeric: tabular-nums; }
    .unit { position: absolute; right: 10px; top: 50%; transform: translateY(-50%); color: var(--text-3); font-size: var(--fs-sm); pointer-events: none; }
    .hold { display: flex; gap: var(--sp-2); align-items: center; }
    .hold input { width: 72px; text-align: right; font-variant-numeric: tabular-nums; }
    .on { display: flex; justify-content: space-between; align-items: center; font-size: var(--fs-base); }
    .err { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--bad-soft); font-size: var(--fs-sm); }
    .err dy-icon { --icon-size: 15px; color: var(--bad); margin-top: 1px; }
  `,
})
export class RuleDialog {
  protected readonly data = inject<RuleDialogData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly metrics = CUSTOM_METRICS;
  protected readonly ops: CustomOp[] = ['above', 'below'];
  protected readonly severities = SEVERITIES;
  protected readonly word = severityWord;

  private readonly start = this.data.rule;
  protected readonly name = signal(this.start?.name ?? '');
  protected readonly metric = signal<CustomMetric>(this.start?.metric ?? 'cpuTotal');
  protected readonly op = signal<CustomOp>(this.start?.op ?? 'above');
  protected readonly limit = signal(String(this.start?.limit ?? 90));
  protected readonly unit = signal<'sec' | 'min'>(this.start && this.start.forSecs > 0 && this.start.forSecs % 60 === 0 ? 'min' : 'sec');
  protected readonly hold = signal(String(this.start ? (this.unit() === 'min' ? this.start.forSecs / 60 : this.start.forSecs) : 120));
  protected readonly severity = signal<Severity>(this.start?.severity ?? 'warning');
  protected readonly enabled = signal(this.start?.enabled ?? true);
  protected readonly busy = signal(false);
  protected readonly engineError = signal<string | null>(null);
  protected readonly info = computed(() => metricInfo(this.metric()));
  /** The first thing wrong with the form (a convenience: the engine validates again), or the engine's own message. */
  protected readonly problem = computed(() => this.submitted() ? this.engineError() ?? this.check() : null);
  private readonly submitted = signal(false);

  private holdSecs(): number {
    const n = Number(this.hold().trim());
    return n * (this.unit() === 'min' ? 60 : 1);
  }

  private check(): string | null {
    if (!this.name().trim()) return 'Give the rule a name.';
    const limit = Number(this.limit().trim().replace(',', '.'));
    if (this.limit().trim() === '' || !Number.isFinite(limit)) return 'The limit must be a number.';
    const secs = this.holdSecs();
    if (this.hold().trim() === '' || !Number.isInteger(secs) || secs < 0 || secs > MAX_HOLD_SECS) return 'The time must be a whole number from 0 to 60 minutes.';
    return null;
  }

  protected setUnit(u: 'sec' | 'min'): void {
    if (u === this.unit()) return;
    const secs = this.holdSecs();
    this.unit.set(u);
    if (Number.isFinite(secs)) this.hold.set(String(u === 'min' ? Math.round((secs / 60) * 100) / 100 : secs));
  }

  protected step(dir: 1 | -1): void {
    const current = Number(this.limit().replace(',', '.'));
    const next = (Number.isFinite(current) ? current : 0) + dir * (this.info().unit === '°C' ? 1 : 5);
    this.limit.set(String(Math.min(this.info().unit === '°C' ? 150 : 100, Math.max(this.info().unit === '°C' ? -50 : 0, next))));
  }

  protected async submit(): Promise<void> {
    this.submitted.set(true);
    this.engineError.set(null);
    if (this.check()) return;
    const name = this.name().trim();
    const rule: CustomAlertRule = {
      id: this.start?.id ?? uniqueRuleId(name, this.data.takenIds),
      name,
      metric: this.metric(),
      op: this.op(),
      limit: Number(this.limit().trim().replace(',', '.')),
      forSecs: this.holdSecs(),
      severity: this.severity(),
      enabled: this.enabled(),
    };
    this.busy.set(true);
    const error = await this.data.save(rule);
    this.busy.set(false);
    if (error) this.engineError.set(error.message);
    else this.ref.close(true);
  }
}
