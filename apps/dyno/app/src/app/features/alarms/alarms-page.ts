import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { AlarmsStore } from '../../state/alarms.store';
import { AlarmsActive } from './alarms-active';
import { AlarmsHistory } from './alarms-history';
import { AlarmsRules } from './alarms-rules';

type Tab = 'active' | 'history' | 'rules';

/** Alarms: what needs attention now (Active), what changed (History) and how alarms are raised and delivered (Rules). */
@Component({
  selector: 'dy-alarms-page',
  imports: [AlarmsActive, AlarmsHistory, AlarmsRules],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles"><h1>Alarms</h1><p>Problems Dyno found, until you acknowledge them.</p></div>
        <div class="seg" role="tablist" aria-label="Alarms">
          @for (t of tabs; track t.id) {
            <button type="button" role="tab" [id]="'alarm-tab-' + t.id" [attr.aria-selected]="tab() === t.id" [attr.aria-controls]="'alarm-panel'" (click)="tab.set(t.id)">{{ t.label }}</button>
          }
        </div>
      </header>
      <div id="alarm-panel" role="tabpanel" [attr.aria-labelledby]="'alarm-tab-' + tab()">
        @switch (tab()) {
          @case ('active') { <dy-alarms-active /> }
          @case ('history') { <dy-alarms-history /> }
          @case ('rules') { <dy-alarms-rules /> }
        }
      </div>
    </div>
  `,
  styles: `:host { display: block; }`,
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
