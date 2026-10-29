import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import type { CustomAlertRule } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { AlarmsStore } from '../../state/alarms.store';
import { SettingsStore } from '../../state/settings.store';
import { TABLE_CSS } from './alarms-css';
import { ruleSummary, severityWord } from './alarms-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from '../../shared/full-text-tip';
import { MatMenuModule } from '@angular/material/menu';
import { MatDivider } from '@angular/material/divider';
import { RowMenu } from '../../shared/row-menu';

/** The id of the rule form's window (its contract is in `src-tauri/src/dialogs.rs`). */
const ALERT_RULE_DIALOG_ID = 'dy.alert-rule';

/** The user's own alert rules from the settings: add, edit, enable and delete. The engine validates every save. */
@Component({
  selector: 'dy-custom-rules',
  imports: [RowMenu, MatMenuModule, MatDivider, MatIconButton, FullTextTip, MatTooltip, MatButton, AsyncState, Icon, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-labelledby="custom-h">
      <div class="sec-head">
        <div class="titles"><h2 class="h2" id="custom-h">Custom alert rules</h2><p class="lead">Raise an alarm when a reading stays past a limit you choose.</p></div>
        <button type="button" matButton="filled" [disabled]="!settings.settings()" (click)="edit(null)"><dy-icon name="plus" /> Add rule</button>
      </div>
      <dy-async [loaded]="!!settings.settings()" [error]="settings.error()" [empty]="rules().length === 0" emptyTitle="No custom rules" emptyText="Add a rule to be told when the CPU, memory, disk or temperature crosses your own limit." emptyIcon="bell" what="alert rules" [rows]="3" (retry)="settings.load()">
        <div class="grid" role="table" aria-label="Custom alert rules">
          <div class="head" role="row">
            <span role="columnheader">Name</span>
            <span role="columnheader">Condition</span>
            <span role="columnheader">Severity</span>
            <span role="columnheader">Enabled</span>
            <span role="columnheader"><span class="visually-hidden">Actions</span></span>
          </div>
          @for (r of rules(); track r.id) {
            <div class="row" role="row" [class.off]="!r.enabled"  dyRowMenu [matContextMenuTriggerFor]="menu" [matContextMenuTriggerData]="{ rule: r }">
              <span class="cell" role="cell"><span class="truncate" [dyFullText]="r.name">{{ r.name }}</span></span>
              <span class="cell muted" role="cell"><span class="truncate" [dyFullText]="summary(r)">{{ summary(r) }}</span></span>
              <span class="cell" role="cell"><span class="dot" [attr.data-tone]="r.severity === 'critical' ? 'bad' : r.severity === 'warning' ? 'warn' : 'info'" aria-hidden="true"></span><span [class.muted]="r.severity !== 'critical'">{{ word(r) }}</span></span>
              <span class="cell" role="cell"><mat-slide-toggle [checked]="r.enabled" [disabled]="alarms.saving()" [aria-label]="'Enable ' + r.name" (change)="toggle(r, $event.checked)" /></span>
              <span class="cell acts" role="cell">
                <button type="button" matIconButton [attr.aria-label]="'Edit ' + r.name" matTooltipPosition="left" [matTooltip]="'Edit ' + r.name" (click)="edit(r)"><dy-icon name="edit" /></button>
                <button type="button" matIconButton class="more" [attr.aria-label]="'More actions for ' + r.name" [matMenuTriggerFor]="menu" [matMenuTriggerData]="{ rule: r }"><dy-icon name="more" /></button>
              </span>
            </div>
          }
        </div>
      </dy-async>
    </section>

    <mat-menu #menu="matMenu" aria-label="Actions">
      <ng-template matMenuContent let-rule="rule">
        <button mat-menu-item (click)="edit(rule)"><dy-icon name="edit" /> Edit…</button>
        <button mat-menu-item (click)="toggle(rule, !rule.enabled)"><dy-icon name="power" /> {{ rule.enabled ? 'Turn off' : 'Turn on' }}</button>
        <mat-divider />
        <button mat-menu-item class="danger" (click)="remove(rule)"><dy-icon name="trash" /> Delete…</button>
      </ng-template>
    </mat-menu>
  `,
  styles: [
    TABLE_CSS,
    `
    :host { display: block; }
    .sec-head { display: flex; align-items: flex-end; justify-content: space-between; gap: var(--sp-3); margin-bottom: var(--sp-3); }
    .titles { display: grid; gap: 2px; }
    .head, .row { --cols: minmax(120px, 1fr) minmax(200px, 1.6fr) 100px 70px 64px; }
    .row { min-height: 40px; }
    .row.off .cell:not(:nth-child(4)):not(.acts) { opacity: 0.55; }
  `,
  ],
})
export class CustomRules {
  protected readonly settings = inject(SettingsStore);
  protected readonly alarms = inject(AlarmsStore);
  private readonly dialogs = inject(DialogService);
  private readonly notifier = inject(Notifier);
  protected readonly rules = computed(() => this.settings.settings()?.customAlerts ?? []);
  protected readonly summary = ruleSummary;
  protected readonly word = (r: CustomAlertRule) => severityWord(r.severity);

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
