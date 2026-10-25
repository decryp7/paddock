import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import type { ProfileId, Rule } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { Switch } from '../../shared/switch';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';
import { RuleDialog, type RuleDialogData } from './rule-dialog';
import { RulesLive } from './rules-live';
import { RulesProtected } from './rules-protected';
import { injectNow } from '../../shared/now';
import { TemporaryDialog } from './temporary-dialog';
import { TEMPORARY_CHOICES, type TemporaryChoice, conditionSummary, duplicateRule, expiryFor, ladderSummary, profilesLabel, remainingText, stepDetails } from './rules-model';

/** CPU taming and profiles: the rules (all off until enabled), what is in effect now, what is protected and what happened. */
@Component({
  selector: 'dy-rules-page',
  imports: [AsyncState, Icon, Switch, MatTooltip, CdkMenu, CdkMenuItem, CdkMenuTrigger, CdkContextMenuTrigger, RulesLive, RulesProtected],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>CPU taming</h1>
          <p>Nothing acts until a rule is enabled.</p>
        </div>
        <div class="seg" role="group" aria-label="Profile">
          @for (p of store.profiles(); track p.id) {
            <button type="button" [attr.aria-pressed]="store.profile() === p.id" [matTooltip]="p.description" [disabled]="store.saving()" (click)="setProfile(p.id)">{{ p.name }}</button>
          }
        </div>
        <span class="pause"><dy-switch [checked]="store.paused()" label="Pause all rules" [disabled]="!store.loaded() || store.saving()" (changed)="store.pause($event)" /> Pause all rules</span>
        <button type="button" class="btn primary" [disabled]="!store.loaded()" (click)="add()"><dy-icon name="plus" /> Add rule</button>
      </header>

      <section class="card" aria-label="Rules">
        <dy-async [loaded]="store.loaded()" [error]="store.error()" [empty]="store.ruleSet()?.rules?.length === 0" emptyTitle="No rules yet" emptyText="A rule lowers an app that keeps the CPU busy, then lets go when it calms down." emptyIcon="rules" what="rules" [rows]="4" (retry)="store.load()">
          <table class="table" aria-label="Rules">
            <thead>
              <tr><th class="on">Enabled</th><th>Rule</th><th>Ladder</th><th>Profiles</th><th class="dur">Duration</th><th class="act"><span class="visually-hidden">Actions</span></th></tr>
            </thead>
            <tbody>
              @for (r of store.ruleSet()?.rules ?? []; track r.id) {
                <tr tabindex="0" [attr.data-id]="r.id" [class.off]="!r.enabled" [cdkContextMenuTriggerFor]="menu" [cdkContextMenuTriggerData]="{ rule: r }" (keydown)="key($event)">
                  <td class="on"><dy-switch [checked]="r.enabled" [label]="'Enable ' + r.name" [disabled]="store.saving()" (changed)="toggle(r, $event)" /></td>
                  <td class="name">
                    <strong class="truncate" [title]="r.name">{{ r.name }}</strong>
                    <small class="truncate" [title]="summary(r)">{{ summary(r) }}</small>
                  </td>
                  <td class="ladder"><span class="truncate" [title]="ladder(r)">{{ ladder(r) }}</span></td>
                  <td class="profiles"><span class="truncate" [title]="profiles(r)">{{ profiles(r) }}</span></td>
                  <td class="dur">
                    <button type="button" class="dur-btn" [class.temp]="r.enabled && r.expiresAt !== null" [disabled]="!r.enabled || store.saving()" [attr.aria-label]="'How long ' + r.name + ' stays on'" [title]="r.enabled ? 'Choose how long this rule stays on' : 'Turn the rule on to make it temporary'" [cdkMenuTriggerFor]="durMenu" [cdkMenuTriggerData]="{ rule: r }">
                      {{ remaining(r) }} <dy-icon name="chevron-down" />
                    </button>
                  </td>
                  <td class="act">
                    <span class="acts">
                      <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Edit ' + r.name" matTooltip="Edit" (click)="edit(r)"><dy-icon name="edit" /></button>
                      <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Duplicate ' + r.name" matTooltip="Duplicate" (click)="duplicate(r)"><dy-icon name="copy" /></button>
                    </span>
                    <button type="button" class="btn ghost icon sm more" data-more [attr.aria-label]="'More actions for ' + r.name" [cdkMenuTriggerFor]="menu" [cdkMenuTriggerData]="{ rule: r }"><dy-icon name="more" /></button>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </dy-async>
      </section>

      <p class="fixed-note"><dy-icon name="shield-check" /> Never touches the app in front; gives it back its CPU the moment you switch to it.</p>

      <dy-rules-live />
      <dy-rules-protected />
    </div>

    <ng-template #durMenu let-rule="rule">
      <div class="menu" cdkMenu [attr.aria-label]="'Duration of ' + rule.name">
        @for (c of choices; track c.id) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="setTemporary(rule, c.id)">{{ c.label }}</button>
        }
      </div>
    </ng-template>

    <ng-template #menu let-rule="rule">
      <div class="menu" cdkMenu [attr.aria-label]="'Actions for ' + rule.name">
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="edit(rule)"><dy-icon name="edit" /> Edit</button>
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="duplicate(rule)"><dy-icon name="copy" /> Duplicate</button>
        <div class="menu-sep" role="separator"></div>
        <button type="button" class="menu-item danger" cdkMenuItem (cdkMenuItemTriggered)="remove(rule)"><dy-icon name="trash" /> Delete…</button>
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 1440px; }
    .page-head { flex-wrap: wrap; }
    .page-head .titles { flex: 1 1 260px; }
    .pause { display: inline-flex; align-items: center; gap: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); }
    .card { overflow: hidden; }
    .table { width: 100%; table-layout: fixed; }
    .table td, .table th { overflow: hidden; text-overflow: ellipsis; }
    .table th:first-child, .table td:first-child { padding-left: var(--sp-4); }
    .on { width: 84px; }
    .name { width: 36%; }
    .ladder { width: 24%; color: var(--text-2); }
    .profiles { width: 12%; color: var(--text-2); }
    .dur { width: 164px; }
    .dur-btn { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; height: 26px; padding: 0 var(--sp-2); border: 0; border-radius: var(--r-sm); background: none; color: var(--text-3); font: var(--fw-regular) var(--fs-sm) / 1 var(--font-sans); cursor: pointer; white-space: nowrap; }
    .dur-btn dy-icon { --icon-size: 13px; opacity: 0.6; }
    .dur-btn:hover:not(:disabled) { background: var(--surface-active); color: var(--text-1); }
    .dur-btn.temp { color: var(--accent-text); font-weight: var(--fw-medium); }
    .dur-btn:disabled { cursor: default; opacity: 0.6; }
    .fixed-note { margin: 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-3); font-size: var(--fs-sm); }
    .fixed-note dy-icon { --icon-size: 15px; }
    .act { width: 112px; text-align: right; padding-right: var(--sp-2) !important; }
    .table tbody tr { outline: none; }
    .table tbody tr:focus-visible td { background: var(--surface-hover); }
    .table tbody tr:focus-visible td:first-child { box-shadow: inset 2px 0 0 var(--accent); }
    .table td.name { height: 52px; }
    .name { white-space: normal; }
    .name strong, .name small { display: block; font-weight: var(--fw-medium); }
    .name small { font-weight: var(--fw-regular); color: var(--text-3); font-size: var(--fs-sm); }
    tr.off .name strong { color: var(--text-2); }
    .truncate { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .act { white-space: nowrap; }
    .acts { display: inline-flex; opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
    tr:hover .acts, tr:focus-within .acts, tr:focus-visible .acts { opacity: 1; }
    .table tbody tr:last-child td { border-bottom: 0; }
  `,
})
export class RulesPage implements OnInit {
  protected readonly store = inject(RulesStore);
  private readonly dialogs = inject(Dialogs);
  private readonly confirmed = new Set<string>();
  private readonly now = injectNow();
  protected readonly choices = TEMPORARY_CHOICES;
  /** Subscribes to the live summary (the "in effect" table reads it). */
  private readonly live = inject(LiveStore);

  protected readonly summary = conditionSummary;
  protected readonly ladder = (r: Rule): string => ladderSummary(r.ladder);
  protected readonly profiles = (r: Rule): string => profilesLabel(r.profiles, this.store.profiles());

  /** "Until turned off" or "Ends in 2 h 10 min". */
  protected readonly remaining = (r: Rule): string => remainingText(r.expiresAt, this.now());

  /** Sets (or clears) the end time of a rule that is on; "Until a time…" asks for the time first. */
  protected async setTemporary(rule: Rule, choice: TemporaryChoice): Promise<void> {
    let time = '';
    if (choice === 'custom') {
      const ref = this.dialogs.open<string | null>(TemporaryDialog);
      const picked = await new Promise<string | null>((resolve) => ref.closed.subscribe((t) => resolve((t as string | null | undefined) ?? null)));
      if (!picked) return;
      time = picked;
    }
    const expiresAt = expiryFor(choice, Date.now(), time);
    await this.store.saveRule({ ...rule, expiresAt });
  }

  ngOnInit(): void {
    void this.store.ensure();
  }

  protected setProfile(id: ProfileId): void {
    if (id !== this.store.profile()) void this.store.setProfile(id);
  }

  protected add(): void {
    this.dialogs.open<boolean, RuleDialogData>(RuleDialog, { rule: null });
  }

  protected edit(rule: Rule): void {
    this.dialogs.open<boolean, RuleDialogData>(RuleDialog, { rule });
  }

  protected duplicate(rule: Rule): void {
    void this.store.saveRule(duplicateRule(rule, this.store.ruleSet()?.rules.map((r) => r.id) ?? []));
  }

  /** Turning a rule off just does it; turning one on says what it will do the first time. */
  protected async toggle(rule: Rule, on: boolean): Promise<void> {
    if (on && !this.confirmed.has(rule.id)) {
      const ok = await this.dialogs.confirm({
        title: `Enable “${rule.name}”?`,
        message: `When ${this.summary(rule)}, Dyno will:`,
        details: [...stepDetails(rule), 'It never touches the foreground app or a protected process.'],
        confirmLabel: 'Enable rule',
        icon: 'rules',
      });
      if (!ok) return;
      this.confirmed.add(rule.id);
    }
    await this.store.toggleRule(rule.id, on);
  }

  protected async remove(rule: Rule): Promise<void> {
    const ok = await this.dialogs.confirm({
      title: `Delete “${rule.name}”?`,
      message: 'Anything this rule is doing to an app is released. This cannot be undone.',
      confirmLabel: 'Delete rule',
      danger: true,
    });
    if (ok) await this.store.deleteRule(rule.id);
  }

  /** Arrows move between rows, Enter edits, Shift+F10 or the Menu key opens the row's menu. */
  protected key(e: KeyboardEvent): void {
    const row = (e.target as HTMLElement).closest<HTMLTableRowElement>('tbody tr');
    if (!row || e.target !== row) return;
    const rows = [...row.parentElement!.querySelectorAll<HTMLTableRowElement>('tr')];
    const i = rows.indexOf(row);
    const rule = this.store.ruleSet()?.rules.find((r) => r.id === row.dataset['id']);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus();
    } else if (e.key === 'Enter' && rule) {
      e.preventDefault();
      this.edit(rule);
    } else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      e.preventDefault();
      row.querySelector<HTMLButtonElement>('[data-more]')?.click();
    }
  }
}
