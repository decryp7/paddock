import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { PdJobsList } from '@decryp7/paddock-ui/shell/jobs';
import { PdCell, PdTable, type PdColumn, type PdRowAction } from '@decryp7/paddock-ui/table';
import { DemoPage, SECTION_STYLES } from './demo-page';
import type { Runner } from '../runner-data';
import { RunnersPageState } from './runners.providers';

const STATE_WORD = { online: 'Online', updating: 'Updating', offline: 'Offline' } as const;

/** A manager page: the fleet as a table and the jobs running on it, with page-scoped state from its own providers. */
@Component({
  selector: 'sd-runners-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdJobsList, PdTable, PdCell],
  host: { '[attr.data-visit]': 'state.visit' },
  template: `
    <sd-page heading="Runners" subtitle="Build machines registered with this workspace.">
      <pd-table label="Runners" [rows]="state.rows()" [columns]="columns" [rowKey]="key" [filterable]="false" [rowActions]="actions" (action)="lastAction.set($event.id + ' ' + $event.row.name)">
        <ng-template pdCell="name" let-r><span class="who"><span class="name" [attr.title]="r.name">{{ r.name }}</span><span class="host" [attr.title]="r.host">{{ r.host }}</span></span></ng-template>
        <ng-template pdCell="state" let-r><span class="state" [attr.data-state]="r.state"><span class="dot" aria-hidden="true"></span>{{ stateWord(r) }}</span></ng-template>
      </pd-table>
      <p class="sr" aria-live="polite" data-testid="runner-action">{{ lastAction() }}</p>
      <section aria-labelledby="sd-jobs">
        <h2 id="sd-jobs">Jobs</h2>
        <pd-jobs-list />
      </section>
    </sd-page>
  `,
  styles:
    SECTION_STYLES +
    `
    /* One line: the table's rows have the family's fixed heights, so the host is secondary text beside the name, and it is the host that gives way first. */
    .who { display: flex; align-items: baseline; gap: var(--pd-space-2); min-width: 0; }
    .name, .host { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .name { flex: 0 1 auto; }
    .host { flex: 0 100000 auto; min-width: 0; }
    .name { font-weight: var(--pd-weight-medium); }
    .host { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .state { display: inline-flex; align-items: center; gap: var(--pd-space-1); }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--pd-text-muted); }
    .state[data-state='online'] .dot { background: var(--pd-success); }
    .state[data-state='updating'] .dot { background: var(--pd-info); }
    .state[data-state='offline'] { color: var(--pd-text-muted); }
    .state[data-state='offline'] .dot { background: transparent; box-shadow: inset 0 0 0 1.5px var(--pd-text-muted); }
    .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class RunnersPage {
  protected readonly state = inject(RunnersPageState);
  protected readonly lastAction = signal('');
  protected stateWord(r: Runner): string {
    return STATE_WORD[r.state];
  }
  protected readonly key = (r: Runner): string => r.name;
  /** Narrow page areas keep the runner and its state; the counts and times are on wider ones (the table drops them by its own width). */
  protected readonly columns: PdColumn<Runner>[] = [
    { key: 'name', header: 'Runner', value: (r) => r.name, required: true, width: 'minmax(0, 2fr)' },
    { key: 'state', header: 'State', value: (r) => r.state, width: '92px' },
    { key: 'jobs', header: 'Jobs today', value: (r) => r.jobsToday, align: 'end', width: '96px', hideBelow: 400 },
    { key: 'when', header: 'Last job', value: (r) => r.lastJob, width: 'minmax(7.5rem, 1fr)', hideBelow: 520 },
  ];
  protected readonly actions = (r: Runner): readonly PdRowAction[] => [
    { id: 'restart', label: 'Restart runner', icon: 'refresh', inline: true },
    { id: 'remove', label: 'Remove…', icon: 'trash', danger: true, disabled: r.state === 'updating' },
  ];
}
