import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdJobsList } from '@decryp7/paddock-ui/shell/jobs';
import { DemoPage, SECTION_STYLES } from './demo-page';
import { RunnersPageState } from './runners.providers';

const STATE_WORD = { online: 'Online', updating: 'Updating', offline: 'Offline' } as const;

/** A manager page: the fleet as a table and the jobs running on it, with page-scoped state from its own providers. */
@Component({
  selector: 'sd-runners-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdJobsList],
  host: { '[attr.data-visit]': 'state.visit' },
  template: `
    <sd-page heading="Runners" subtitle="Build machines registered with this workspace.">
      <table>
        <caption class="sr">Runners</caption>
        <thead>
          <tr>
            <th scope="col">Runner</th>
            <th scope="col">State</th>
            <th scope="col" class="num jobs">Jobs today</th>
            <th scope="col" class="when">Last job</th>
          </tr>
        </thead>
        <tbody>
          @for (r of state.rows(); track r.name) {
            <tr>
              <td><span class="name">{{ r.name }}</span><span class="host">{{ r.host }}</span></td>
              <td><span class="state" [attr.data-state]="r.state"><span class="dot" aria-hidden="true"></span>{{ word[r.state] }}</span></td>
              <td class="num jobs">{{ r.jobsToday }}</td>
              <td class="when">{{ r.lastJob }}</td>
            </tr>
          }
        </tbody>
      </table>
      <section aria-labelledby="sd-jobs">
        <h2 id="sd-jobs">Jobs</h2>
        <pd-jobs-list />
      </section>
    </sd-page>
  `,
  styles:
    SECTION_STYLES +
    `
    td { height: var(--pd-row-height); white-space: nowrap; }
    .name { display: block; font-weight: var(--pd-weight-medium); }
    .host { display: block; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .state { display: inline-flex; align-items: center; gap: var(--pd-space-1); }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--pd-text-muted); }
    .state[data-state='online'] .dot { background: var(--pd-success); }
    .state[data-state='updating'] .dot { background: var(--pd-info); }
    .state[data-state='offline'] { color: var(--pd-text-muted); }
    .state[data-state='offline'] .dot { background: transparent; box-shadow: inset 0 0 0 1.5px var(--pd-text-muted); }
    .when { color: var(--pd-text-muted); }
    .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    /* Narrow page areas keep the runner and its state; the counts and times are on wider ones. */
    @container pd-page (max-width: 520px) { .when { display: none; } }
    @container pd-page (max-width: 400px) { .jobs { display: none; } }
  `,
})
export class RunnersPage {
  protected readonly state = inject(RunnersPageState);
  protected readonly word = STATE_WORD;
}
