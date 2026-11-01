import { ChangeDetectionStrategy, Component, computed, inject, input, signal, type OnInit } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import type { CustomAlertRule, CustomMetric, CustomOp, Severity } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Icon } from '../../shared/icon';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { CUSTOM_METRICS, metricInfo, severityWord, uniqueRuleId } from './alarms-model';
import { MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';

const SEVERITIES: Severity[] = ['info', 'warning', 'critical'];
const MAX_HOLD_SECS = 3600;

/** Add or edit a custom alert rule: a metric, above or below a limit, held for a while, with a severity. */
@Component({
  selector: 'dy-rule-dialog',
  imports: [MatTooltip, MatIconButton, PdField, PdFieldControl, PdSelect, SuSegmentedControl, Icon, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
      <p class="lead">Raises an alarm when a reading stays past a limit.</p>
      <pd-field label="Name">
        <input pdFieldControl aria-label="Name" type="text" maxlength="60" placeholder="High CPU while idle" [value]="name()" (input)="name.set($any($event.target).value)" />
      </pd-field>
      <pd-select class="reading" label="Reading" ariaLabel="Reading" describedBy="rule-reading-hint" [options]="metricChoices" [value]="metric()" (valueChange)="setMetric($event)" />
      <p class="hint" id="rule-reading-hint">{{ info().hint }}</p>
      <div class="two">
        <div class="seg-field">
          <span class="seg-label" aria-hidden="true">Alarm when it is</span>
          <pd-segmented-control label="Alarm when it is" [options]="opSegs" [value]="op()" (valueChange)="setOp($event)" />
        </div>
        <pd-field label="Limit" [suffix]="limitTools">
          <input pdFieldControl aria-label="Limit" type="text" inputmode="decimal" [value]="limit()" (input)="limit.set($any($event.target).value)" />
        </pd-field>
      </div>
      <div class="two">
        <div class="hold">
          <pd-field label="For at least" hint="0 raises the alarm right away. At most 60 min.">
            <input pdFieldControl aria-label="Duration" type="text" inputmode="numeric" [value]="hold()" (input)="hold.set($any($event.target).value)" />
          </pd-field>
          <pd-segmented-control class="unit-seg" label="Duration unit" [options]="unitSegs" [value]="unit()" (valueChange)="setUnit($event === 'min' ? 'min' : 'sec')" />
        </div>
        <div class="seg-field">
          <span class="seg-label" aria-hidden="true">Severity</span>
          <pd-segmented-control label="Severity" [options]="severitySegs" [value]="severity()" (valueChange)="setSeverity($event)" />
        </div>
      </div>
      <mat-slide-toggle [checked]="enabled()" (change)="enabled.set($event.checked)">Enabled</mat-slide-toggle>
      @if (problem(); as p) {
        <p class="err" role="alert"><dy-icon name="alert" /> {{ p }}</p>
      }
      <ng-template #limitTools>
        <span class="tools">
          <span class="unit">{{ info().unit }}</span>
          <button type="button" matIconButton matTooltip="Decrease the limit" aria-label="Decrease the limit" (click)="step(-1)"><dy-icon name="minus" /></button>
          <button type="button" matIconButton matTooltip="Increase the limit" aria-label="Increase the limit" (click)="step(1)"><dy-icon name="plus" /></button>
        </span>
      </ng-template>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); min-width: 0; }
    .lead { color: var(--text-2); margin: 0; }
    .reading { display: flex; width: 100%; }
    .reading :is(mat-form-field) { width: 100%; }
    .hint { margin: calc(-1 * var(--sp-2)) 0 0; padding-inline: var(--sp-4); color: var(--text-3); font-size: var(--fs-xs); }
    .hint + .two { margin-top: var(--sp-2); }
    .two { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: var(--sp-4); align-items: start; }
    /* A segmented control labelled like an outline field: the label sits on its top border, and it is as tall as a labelled field (the shared form-field height), so a
       row's controls share top and bottom. The label is decoration (the control carries the accessible name). */
    .seg-field { position: relative; height: var(--mat-form-field-container-height); min-width: 0; --mat-button-toggle-height: calc(var(--mat-form-field-container-height) - 2px); }
    .seg-field pd-segmented-control { display: block; }
    .seg-label { position: absolute; top: -8px; left: var(--sp-3); z-index: 1; padding: 0 var(--sp-1); background: var(--pd-surface-2); color: var(--text-2); font-size: var(--fs-xs); line-height: 1; }
    /* The segments sit 3 px inside the control's border, so the selected one's ring starts below the legend's notch (the legend stays centred
       on the border, like the outline fields' legends). */
    .seg-field pd-segmented-control ::ng-deep .mat-button-toggle,
    .unit-seg ::ng-deep .mat-button-toggle { margin: 3px; --mat-button-toggle-height: calc(var(--pd-field-height, 28px) - 8px); }
    .hold { display: flex; align-items: flex-start; gap: var(--sp-2); min-width: 0; }
    .hold pd-field { flex: 1; min-width: 0; }
    /* The hint is one line that may run on under the unit toggle (the row's width), not a column of wrapped words. */
    .hold pd-field ::ng-deep .mat-mdc-form-field-subscript-wrapper { overflow: visible; }
    .hold pd-field ::ng-deep .mat-mdc-form-field-hint-wrapper { overflow: visible; }
    .hold pd-field ::ng-deep .pd-field__hint { white-space: nowrap; }
    .unit-seg { --mat-button-toggle-height: calc(var(--mat-form-field-container-height) - 2px); }
    .tools { display: inline-flex; align-items: center; gap: var(--sp-1); }
    .tools dy-icon { --icon-size: 14px; }
    .unit { color: var(--text-3); font-size: var(--fs-sm); }
    .err { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--bad-soft); font-size: var(--fs-sm); }
    .err dy-icon { --icon-size: 15px; color: var(--bad); margin-top: 1px; }
  `,
})
export class RuleDialog implements DialogContent<true>, OnInit {
  /** The rule to edit, or null for a new one. */
  readonly rule = input<CustomAlertRule | null>(null);
  /** Ids of the other rules (a new id must avoid them). */
  readonly takenIds = input<readonly string[]>([]);
  private readonly backend = inject(BACKEND);
  protected readonly metricChoices: PdSelectOption[] = CUSTOM_METRICS.map((m) => ({ value: m.id, label: `${m.label} (${m.unit})` }));
  protected readonly opSegs: SuSegment[] = [{ value: 'above', label: 'Above' }, { value: 'below', label: 'Below' }];
  protected readonly unitSegs: SuSegment[] = [{ value: 'sec', label: 'sec' }, { value: 'min', label: 'min' }];
  protected readonly severitySegs: SuSegment[] = SEVERITIES.map((s) => ({ value: s, label: severityWord(s) }));

  /** Cancel, and the rule's one affirmative: the engine validates the rule and stores it; a refusal is shown in the frame with Try again. */
  readonly dialogActions = computed<readonly DialogAction<true>[]>(() => [
    { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
    { kind: 'answer', id: 'save', label: this.rule() ? 'Save' : 'Add rule', role: 'primary', answer: () => this.save() },
  ]);

  protected readonly name = signal('');
  protected readonly metric = signal<CustomMetric>('cpuTotal');
  protected readonly op = signal<CustomOp>('above');
  protected readonly limit = signal('90');
  protected readonly unit = signal<'sec' | 'min'>('sec');
  protected readonly hold = signal('120');
  protected readonly severity = signal<Severity>('warning');
  protected readonly enabled = signal(true);
  protected readonly info = computed(() => metricInfo(this.metric()));
  /** The first thing wrong with the form (a convenience: the engine validates again), or the engine's own message. */
  protected readonly problem = computed(() => (this.submitted() ? this.check() : null));
  private readonly submitted = signal(false);

  /** The inputs are set before this runs: the form starts from the rule being edited. */
  ngOnInit(): void {
    const r = this.rule();
    if (!r) return;
    this.name.set(r.name);
    this.metric.set(r.metric);
    this.op.set(r.op);
    this.limit.set(String(r.limit));
    const minutes = r.forSecs > 0 && r.forSecs % 60 === 0;
    this.unit.set(minutes ? 'min' : 'sec');
    this.hold.set(String(minutes ? r.forSecs / 60 : r.forSecs));
    this.severity.set(r.severity);
    this.enabled.set(r.enabled);
  }

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

  protected setMetric(id: string): void {
    const m = CUSTOM_METRICS.find((x) => x.id === id);
    if (m) this.metric.set(m.id);
  }
  protected setOp(value: string): void {
    this.op.set(value === 'below' ? 'below' : 'above');
  }
  protected setSeverity(value: string): void {
    const s = SEVERITIES.find((x) => x === value);
    if (s) this.severity.set(s);
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

  /** Validates (the engine does again) and stores the rule; a refusal reaches the frame as the dialog's inline error. */
  private async save(): Promise<true> {
    this.submitted.set(true);
    const problem = this.check();
    if (problem) throw new DialogActionError(problem);
    const name = this.name().trim();
    const rule: CustomAlertRule = {
      id: this.rule()?.id ?? uniqueRuleId(name, [...this.takenIds()]),
      name,
      metric: this.metric(),
      op: this.op(),
      limit: Number(this.limit().trim().replace(',', '.')),
      forSecs: this.holdSecs(),
      severity: this.severity(),
      enabled: this.enabled(),
    };
    try {
      await this.backend.upsertAlertRule(rule);
    } catch (e) {
      throw new DialogActionError(toCommandError(e).message);
    }
    return true;
  }
}
