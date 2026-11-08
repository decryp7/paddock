import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import type { CustomAlertRule } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { MatButton } from '@angular/material/button';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { AlarmsStore } from '../../state/alarms.store';
import { SettingsStore } from '../../state/settings.store';
import { ruleSummary, severityWord } from './alarms-model';

/** The id of the rule form's window (its contract is in `src-tauri/src/dialogs.rs`). */
const ALERT_RULE_DIALOG_ID = 'dy.alert-rule';

/** The user's own alert rules from the settings: add, edit, enable and delete. The engine validates every save. */
@Component({
  selector: 'dy-custom-rules',
  imports: [MatButton, AsyncState, Icon, MatSlideToggle, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-labelledby="custom-h">
      <div class="sec-head">
        <div class="titles"><h2 class="h2" id="custom-h">Custom alert rules</h2><p class="lead">Raise an alarm when a reading stays past a limit you choose.</p></div>
        <button type="button" matButton="filled" [disabled]="!settings.settings()" (click)="edit(null)"><dy-icon name="plus" /> Add rule</button>
      </div>
      <dy-async [loaded]="!!settings.settings()" [error]="settings.error()" [empty]="rules().length === 0" emptyTitle="No custom rules" emptyText="Add a rule to be told when the CPU, memory, disk or temperature crosses your own limit." emptyIcon="bell" what="alert rules" [rows]="3" (retry)="settings.load()">
        <pd-table [toolbar]="false" label="Custom alert rules" [rows]="rules()" [columns]="columns" [rowKey]="ruleKey" [filterable]="false" [rowActions]="actions" height="window" (action)="run($event)">
          <ng-template pdCell="severity" let-r><span class="dot" [attr.data-tone]="tone(r)" aria-hidden="true"></span><span [class.muted]="r.severity !== 'critical'">{{ word(r) }}</span></ng-template>
          <ng-template pdCell="enabled" let-r><mat-slide-toggle [checked]="r.enabled" [disabled]="alarms.saving()" [aria-label]="'Enable ' + r.name" (change)="toggle(r, $event.checked)" /></ng-template>
        </pd-table>
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; }
    .sec-head { display: flex; align-items: flex-end; justify-content: space-between; gap: var(--sp-3); margin-bottom: var(--sp-3); }
    .titles { display: grid; gap: 2px; }
    .muted { color: var(--text-2); }
    .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; margin-inline-end: var(--sp-2); background: var(--text-3); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='info'] { background: var(--info); }
  `,
})
export class CustomRules {
  protected readonly settings = inject(SettingsStore);
  protected readonly alarms = inject(AlarmsStore);
  private readonly dialogs = inject(DialogService);
  private readonly notifier = inject(Notifier);
  protected readonly rules = computed(() => this.settings.settings()?.customAlerts ?? []);
  protected readonly summary = ruleSummary;
  protected readonly word = (r: CustomAlertRule) => severityWord(r.severity);
  protected readonly tone = (r: CustomAlertRule): string => (r.severity === 'critical' ? 'bad' : r.severity === 'warning' ? 'warn' : 'info');
  protected readonly ruleKey = (r: CustomAlertRule): string => r.id;
  protected readonly columns: PdColumn<CustomAlertRule>[] = [
    { key: 'name', header: 'Name', value: (r) => r.name, required: true, sortable: false, width: 'minmax(120px, 1fr)' },
    { key: 'condition', header: 'Condition', value: (r) => ruleSummary(r), sortable: false, width: 'minmax(200px, 1.6fr)', hideBelow: 520 },
    { key: 'severity', header: 'Severity', value: (r) => severityWord(r.severity), sortable: false, width: '100px' },
    { key: 'enabled', header: 'Enabled', value: (r) => r.enabled, text: (r) => (r.enabled ? 'On' : 'Off'), sortable: false, width: '80px' },
  ];
  protected readonly actions = (r: CustomAlertRule): PdRowAction[] => [
    { id: 'edit', label: 'Edit…', icon: 'edit', inline: true },
    { id: 'toggle', label: r.enabled ? 'Turn off' : 'Turn on' },
    { id: 'delete', label: 'Delete…', icon: 'trash', danger: true },
  ];

  protected run({ id, row }: PdRowActionEvent<CustomAlertRule>): void {
    if (id === 'edit') void this.edit(row);
    else if (id === 'toggle') void this.toggle(row, !row.enabled);
    else void this.remove(row);
  }

  /** Opens the rule's window (a real OS window, ADR 0026); once the rule is stored the settings are read again. */
  protected async edit(rule: CustomAlertRule | null): Promise<void> {
    const outcome = await this.dialogs.open<true>(ALERT_RULE_DIALOG_ID, {
      inputs: { rule, takenIds: this.rules().map((r) => r.id) },
      title: rule ? 'Edit alert rule' : 'Add alert rule',
    }).closed;
    if (outcome.kind === 'answered') await this.settings.load();
  }

  protected async toggle(rule: CustomAlertRule, enabled: boolean): Promise<void> {
    const error = await this.alarms.upsertRule({ ...rule, enabled });
    if (error) this.notifier.error(error);
  }

  protected async remove(rule: CustomAlertRule): Promise<void> {
    const yes = await confirm(this.dialogs, {
      heading: `Delete “${rule.name}”?`,
      message: 'The rule stops raising alarms. Alarms it already raised stay until you acknowledge them.',
      details: [this.summary(rule)],
      confirm: { label: 'Delete rule', destructive: true },
    });
    if (!yes) return;
    const error = await this.alarms.deleteRule(rule.id);
    if (error) this.notifier.error(error);
  }
}
