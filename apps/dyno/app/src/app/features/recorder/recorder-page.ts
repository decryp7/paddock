import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { Switch } from '../../shared/switch';
import { RecorderStore } from '../../state/recorder.store';
import { SettingsStore } from '../../state/settings.store';
import { SnapshotList } from './snapshot-list';
import { TimelinePanel } from './timeline-panel';

/** The flight recorder: saved snapshots of the last ten minutes and a shared-axis timeline of the selected one. */
@Component({
  selector: 'dy-recorder-page',
  imports: [AsyncState, Icon, Switch, SnapshotList, TimelinePanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Flight recorder</h1>
          <p>The last ten minutes at one-second resolution, kept in memory and saved when an alarm fires or you capture.</p>
        </div>
        <div class="actions">
          <div class="rec">
            <dy-switch [checked]="recording()" [disabled]="settings.settings() === null || settings.saving()" label="Record continuously" (changed)="setRecording($event)" />
            <span aria-hidden="true">Record continuously</span>
          </div>
          <button type="button" class="btn primary" [disabled]="store.capturing()" (click)="store.capture()"><dy-icon name="camera" /> Capture now</button>
        </div>
      </header>

      @if (!recording() && settings.settings() !== null) {
        <p class="note" role="status"><dy-icon name="info" /> Recording is off, so an alarm cannot save the minutes before it. Snapshots you capture by hand still work.</p>
      }

      <dy-async [loaded]="store.listLoaded()" [error]="store.listError()" [empty]="store.snapshots().length === 0" emptyIcon="camera" emptyTitle="Nothing saved yet" emptyText="Nothing saved yet. Capture now, or Dyno saves the last ten minutes when an alarm fires." what="snapshots" [rows]="5" (retry)="store.retry()">
        <div class="layout">
          <section class="card list" aria-label="Snapshots">
            <dy-snapshot-list [snapshots]="store.snapshots()" [selectedId]="store.selectedId()" (picked)="store.select($event)" />
          </section>
          <section class="main" aria-label="Selected snapshot" [attr.aria-busy]="store.snapshot() === null && store.snapshotError() === null">
            <dy-async [loaded]="store.snapshot() !== null" [error]="store.snapshotError()" what="the timeline" [rows]="0" (retry)="store.retry()">
              <div skeleton class="sk skeleton" aria-hidden="true"></div>
              @if (store.snapshot(); as snap) {
                <dy-timeline-panel [snapshot]="snap" [moment]="store.moment()" (picked)="store.setMoment($event)" (step)="store.stepMoment($event)" (peak)="store.jumpToPeak()" />
              }
            </dy-async>
          </section>
        </div>
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .actions { display: flex; align-items: center; gap: var(--sp-4); margin-left: auto; }
    .rec { display: inline-flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-base); color: var(--text-2); }
    .note { display: flex; align-items: center; gap: var(--sp-2); margin: 0; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--surface-1); border-left: 3px solid var(--warn); color: var(--text-2); font-size: var(--fs-sm); }
    .note dy-icon { --icon-size: 15px; color: var(--text-3); }
    .layout { display: grid; grid-template-columns: minmax(300px, 360px) minmax(0, 1fr); gap: var(--sp-4); align-items: start; }
    .list { overflow: hidden; padding: 0; }
    .sk { height: 420px; border-radius: var(--r-md); }
    @media (max-width: 900px) { .layout { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class RecorderPage implements OnInit {
  protected readonly store = inject(RecorderStore);
  protected readonly settings = inject(SettingsStore);
  /** Defaults to on, like the engine, until the settings arrive. */
  protected readonly recording = computed(() => this.settings.settings()?.flightRecorder ?? true);

  ngOnInit(): void {
    void this.store.load();
    if (this.settings.settings() === null) void this.settings.load();
  }

  protected async setRecording(on: boolean): Promise<void> {
    const current = this.settings.settings();
    if (current) await this.settings.save({ ...current, flightRecorder: on }, on ? 'Recording continuously' : 'Recording stopped');
  }
}
