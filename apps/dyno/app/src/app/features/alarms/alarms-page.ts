import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { AlarmsStore } from '../../state/alarms.store';
import { AlarmsActive } from './alarms-active';
import { AlarmsHistory } from './alarms-history';
import { AlarmsRules } from './alarms-rules';
import { MatTabsModule } from '@angular/material/tabs';

type Tab = 'active' | 'history' | 'rules';

/** Alarms: what needs attention now (Active), what changed (History) and how alarms are raised and delivered (Rules). */
@Component({
  selector: 'dy-alarms-page',
  imports: [MatTabsModule, AlarmsActive, AlarmsHistory, AlarmsRules],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page" [class.table-page]="tab() !== 'rules'">
      <header class="page-head">
        <div class="titles"><h1>Alarms</h1><p>Problems Dyno found, until you acknowledge them.</p></div>
        <nav mat-tab-nav-bar [tabPanel]="panel" aria-label="Alarms">
          @for (t of tabs; track t.id) {
            <button type="button" mat-tab-link [active]="tab() === t.id" (click)="tab.set(t.id)">{{ t.label }}</button>
          }
        </nav>
      </header>
      <mat-tab-nav-panel #panel>
        @switch (tab()) {
          @case ('active') { <dy-alarms-active /> }
          @case ('history') { <dy-alarms-history /> }
          @case ('rules') { <dy-alarms-rules /> }
        }
      </mat-tab-nav-panel>
    </div>
  `,
  styles: `
    :host { display: block; height: 100%; }
    /* Active and History are one table each: the whole page area, the table fills the height left. Rules is a set of sections. */
    .page.table-page { display: flex; flex-direction: column; max-width: none; min-height: 100%; padding-bottom: var(--sp-4); }
    .page.table-page > * { flex: none; }
    .page.table-page > mat-tab-nav-panel { flex: 1 1 auto; display: flex; flex-direction: column; }
  `,
})
export class AlarmsPage {
  protected readonly tab = signal<Tab>('active');
  protected readonly tabs: { id: Tab; label: string }[] = [
    { id: 'active', label: 'Active' },
    { id: 'history', label: 'History' },
    { id: 'rules', label: 'Rules' },
  ];

  constructor() {
    void inject(AlarmsStore).load();
  }
}
