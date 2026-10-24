import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { PdPauseEditor, PdScheduleEditor, type PauseWindows, type Schedule } from '@decryp7/paddock-ui/schedule';
import { DemoPage, SECTION_STYLES } from './demo-page';
import { DEMO_NOW, mockPause, mockSchedule } from './schedule-mock';

/** The shared schedule editors (plan X33) on the demo's stand-in engine: a schedule with its preview, and pause windows. */
@Component({
  selector: 'sd-schedule-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdPauseEditor, PdScheduleEditor],
  template: `
    <sd-page heading="Schedules" subtitle="When something runs: every so often, at times of day or by cron, and when it waits.">
      <section aria-labelledby="sd-schedule">
        <h2 id="sd-schedule">Schedule</h2>
        <p class="context" data-testid="schedule-now">The demo clock stands at {{ now }}.</p>
        <pd-schedule-editor [(value)]="schedule" [preview]="answer().preview" [problems]="answer().problems" locale="en-GB" />
      </section>
      <section aria-labelledby="sd-pause">
        <h2 id="sd-pause">Pause windows</h2>
        <pd-pause-editor [(value)]="pause" [problems]="pauseProblems()" />
      </section>
    </sd-page>
  `,
  styles: SECTION_STYLES,
})
export class SchedulePage {
  protected readonly now = new Date(DEMO_NOW).toISOString();
  protected readonly schedule = signal<Schedule>({ mode: 'every', secs: 3600 });
  protected readonly pause = signal<PauseWindows>({ windows: [{ days: [], start: '22:00', end: '06:00' }], zone: null });
  protected readonly answer = computed(() => mockSchedule(this.schedule()));
  protected readonly pauseProblems = computed(() => mockPause(this.pause()));
}
