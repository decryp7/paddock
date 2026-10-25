import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { ProfileId, Rule, RuleStep } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { RulesStore } from '../../state/rules.store';
import { STEP_KINDS, type StepKind, blankRule, newStep, parseApps, ruleId, stepKind, validateRule } from './rules-model';

export interface RuleDialogData {
  /** The rule to change, or `null` to create one. */
  rule: Rule | null;
}

/** Creates or edits a rule. Saves through the store and shows the engine's message inline when it refuses. */
@Component({
  selector: 'dy-rule-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="data.rule ? 'Edit rule' : 'Add rule'" subtitle="Dyno steps in only while the condition holds. It never touches the foreground app or a protected process." icon="rules">
      <div class="field">
        <label for="rule-name">Name</label>
        <input id="rule-name" class="input" type="text" maxlength="80" [value]="draft().name" (input)="patch({ name: val($event) })" />
      </div>

      <fieldset class="group">
        <legend>When</legend>
        <div class="seg" role="group" aria-label="Target">
          <button type="button" [attr.aria-pressed]="!specific()" (click)="setTarget(false)">Any app</button>
          <button type="button" [attr.aria-pressed]="specific()" (click)="setTarget(true)">Specific apps</button>
        </div>
        @if (specific()) {
          <div class="field">
            <label for="rule-apps">Apps (executable names, comma or line separated)</label>
            <textarea id="rule-apps" class="input" rows="2" placeholder="chrome.exe, msedge.exe" [value]="apps()" (input)="setApps(val($event))"></textarea>
          </div>
        }
        <div class="pair">
          <label class="field"><span class="label">CPU above</span><span class="num-in"><input class="input" type="number" min="1" max="100" aria-label="CPU above percent" [value]="draft().condition.cpuAbove" (input)="cond({ cpuAbove: num($event) })" /><span>%</span></span></label>
          <label class="field"><span class="label">For at least</span><span class="num-in"><input class="input" type="number" min="0" aria-label="For seconds" [value]="draft().condition.forSecs" (input)="cond({ forSecs: num($event) })" /><span>seconds</span></span></label>
        </div>
      </fieldset>

      <fieldset class="group">
        <legend>Then Dyno</legend>
        <ol class="ladder">
          @for (s of draft().ladder; track $index; let i = $index) {
            <li>
              <span class="n">{{ i + 1 }}</span>
              <select class="select" [attr.aria-label]="'Step ' + (i + 1) + ' action'" (change)="setKind(i, val($event))">
                @for (k of kinds; track k.kind) {
                  <option [value]="k.kind" [selected]="kind(s) === k.kind">{{ k.label }}</option>
                }
              </select>
              @if (cap(s) !== null) {
                <span class="num-in"><input class="input" type="number" min="1" max="99" [attr.aria-label]="'Step ' + (i + 1) + ' cap percent'" [value]="cap(s)" (input)="setCap(i, num($event))" /><span>%</span></span>
              }
              <span class="num-in"><input class="input" type="number" min="0" [attr.aria-label]="'Step ' + (i + 1) + ' delay seconds'" [value]="s.afterSecs" (input)="setAfter(i, num($event))" /><span>s after</span></span>
              <span class="ops">
                <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Move step ' + (i + 1) + ' up'" [disabled]="i === 0" (click)="move(i, -1)"><dy-icon name="arrow-up" /></button>
                <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Move step ' + (i + 1) + ' down'" [disabled]="i === draft().ladder.length - 1" (click)="move(i, 1)"><dy-icon name="arrow-down" /></button>
                <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Remove step ' + (i + 1)" (click)="remove(i)"><dy-icon name="close" /></button>
              </span>
            </li>
          }
        </ol>
        <div><button type="button" class="btn sm" (click)="add()"><dy-icon name="plus" /> Add step</button></div>
        <label class="field"><span class="label">Release after</span><span class="num-in"><input class="input" type="number" min="0" aria-label="Release after seconds" [value]="draft().releaseAfterSecs" (input)="patch({ releaseAfterSecs: num($event) })" /><span>seconds below the threshold</span></span></label>
      </fieldset>

      <fieldset class="group">
        <legend>Profiles</legend>
        <div class="chips" role="group" aria-label="Profiles">
          <button type="button" class="tog" [attr.aria-pressed]="draft().profiles.length === 0" (click)="patch({ profiles: [] })">All profiles</button>
          @for (p of store.profiles(); track p.id) {
            <button type="button" class="tog" [attr.aria-pressed]="draft().profiles.includes(p.id)" (click)="toggleProfile(p.id)">{{ p.name }}</button>
          }
        </div>
      </fieldset>

      @if (problem(); as p) {
        <p class="err" role="alert"><dy-icon name="alert" /> {{ p }}</p>
      }

      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(false)">Cancel</button>
        <button type="button" class="btn primary" [disabled]="store.saving()" (click)="save()">{{ data.rule ? 'Save' : 'Add rule' }}</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 560px; }
    .group { display: grid; gap: var(--sp-3); margin: 0; padding: var(--sp-3) 0 0; border: 0; border-top: 1px solid var(--border-subtle); }
    legend { padding: 0; margin-bottom: var(--sp-2); float: left; width: 100%; font-size: var(--fs-sm); font-weight: var(--fw-medium); color: var(--text-2); }
    legend + * { clear: both; }
    .seg { justify-self: start; }
    .pair { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-3); }
    .num-in { display: inline-flex; align-items: center; gap: var(--sp-2); color: var(--text-3); font-size: var(--fs-sm); white-space: nowrap; }
    .num-in .input { width: 76px; }
    .ladder { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    .ladder li { display: flex; align-items: center; gap: var(--sp-2); }
    .n { width: 18px; color: var(--text-3); font-size: var(--fs-sm); text-align: center; flex: none; }
    .ladder .select { width: 150px; flex: none; }
    .ops { margin-left: auto; display: inline-flex; flex: none; }
    .chips { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
    .tog { display: inline-flex; align-items: center; height: var(--control-h); padding: 0 10px; border: 1px solid var(--border-strong); border-radius: var(--r-pill); background: var(--surface-2); color: var(--text-2); font: var(--fw-medium) var(--fs-sm) / 1 var(--font-sans); cursor: pointer; }
    .tog[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    .err { display: flex; align-items: flex-start; gap: var(--sp-2); margin: 0; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--bad-soft); color: var(--text-1); font-size: var(--fs-sm); }
    .err dy-icon { --icon-size: 15px; color: var(--bad); }
  `,
})
export class RuleDialog {
  protected readonly data = inject<RuleDialogData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly store = inject(RulesStore);
  protected readonly kinds = STEP_KINDS;
  protected readonly draft = signal<Rule>(
    this.data.rule ? structuredClone(this.data.rule) : blankRule(ruleId('new rule', this.store.ruleSet()?.rules.map((r) => r.id) ?? [])),
  );
  protected readonly specific = computed(() => this.draft().condition.target !== 'anyApp');
  protected readonly apps = signal(this.specificApps(this.draft()));
  private readonly local = signal<string | null>(null);
  protected readonly problem = computed(() => this.local() ?? this.store.saveError()?.message ?? null);

  private specificApps(r: Rule): string {
    return r.condition.target === 'anyApp' ? '' : r.condition.target.apps.join(', ');
  }

  protected val(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }
  protected num(e: Event): number {
    const v = (e.target as HTMLInputElement).valueAsNumber;
    return Number.isFinite(v) ? v : Number.NaN;
  }
  protected kind = (s: RuleStep): StepKind => stepKind(s.action);
  protected cap = (s: RuleStep): number | null => (typeof s.action === 'string' ? null : s.action.cpuCap);

  protected patch(p: Partial<Rule>): void {
    this.draft.update((d) => ({ ...d, ...p }));
  }
  protected cond(p: Partial<Rule['condition']>): void {
    this.draft.update((d) => ({ ...d, condition: { ...d.condition, ...p } }));
  }
  protected setTarget(specific: boolean): void {
    this.cond({ target: specific ? { apps: parseApps(this.apps()) } : 'anyApp' });
  }
  protected setApps(text: string): void {
    this.apps.set(text);
    this.cond({ target: { apps: parseApps(text) } });
  }
  private steps(f: (s: RuleStep[]) => RuleStep[]): void {
    this.draft.update((d) => ({ ...d, ladder: f([...d.ladder]) }));
  }
  protected add(): void {
    this.steps((s) => [...s, newStep('efficiencyMode', 60)]);
  }
  protected remove(i: number): void {
    this.steps((s) => s.filter((_, j) => j !== i));
  }
  protected move(i: number, by: -1 | 1): void {
    this.steps((s) => {
      const [x] = s.splice(i, 1);
      s.splice(i + by, 0, x!);
      return s;
    });
  }
  protected setKind(i: number, kind: string): void {
    this.steps((s) => s.map((x, j) => (j === i ? newStep(kind as StepKind, x.afterSecs) : x)));
  }
  protected setCap(i: number, cpuCap: number): void {
    this.steps((s) => s.map((x, j) => (j === i ? { ...x, action: { cpuCap } } : x)));
  }
  protected setAfter(i: number, afterSecs: number): void {
    this.steps((s) => s.map((x, j) => (j === i ? { ...x, afterSecs } : x)));
  }
  protected toggleProfile(id: ProfileId): void {
    const cur = this.draft().profiles;
    this.patch({ profiles: cur.includes(id) ? cur.filter((p) => p !== id) : [...cur, id] });
  }

  protected async save(): Promise<void> {
    const rule = this.draft();
    const problem = validateRule(rule);
    this.local.set(problem);
    if (problem) return;
    const taken = this.store.ruleSet()?.rules.map((r) => r.id) ?? [];
    const toSave = this.data.rule ? rule : { ...rule, id: ruleId(rule.name, taken) };
    if (await this.store.saveRule({ ...toSave, name: rule.name.trim() }, true)) this.ref.close(true);
  }
}
