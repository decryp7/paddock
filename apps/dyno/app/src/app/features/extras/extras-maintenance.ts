import { ChangeDetectionStrategy, Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import type { MaintenanceSettings, MaintenanceWindow, Weekday } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { ExtrasStore } from '../../state/extras.store';
import { SettingsStore } from '../../state/settings.store';
import { EXTRAS_STYLES } from './extras-styles';
import {
  MAX_WINDOWS,
  WEEKDAYS,
  addWindow,
  minuteToTime,
  removeWindow,
  statusText,
  timeToMinute,
  validateMaintenance,
  withWindow,
  windowText,
} from './maintenance-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { PdDayPicker } from '@decryp7/paddock-ui/schedule';
import { TimeField } from '../../shared/time-field';
import { MatTooltip } from '@angular/material/tooltip';

/**
 * Maintenance windows: when Dyno's own heavier background work (scans, backup checks, report building) may run. Clicks
 * always run at once. Saved through the settings; the engine validates the windows and its sentence is shown when it refuses.
 */
@Component({
  selector: 'dy-extras-maintenance',
  imports: [MatTooltip, TimeField, PdDayPicker, MatButton, MatIconButton, AsyncState, Icon, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!draft()" [error]="settings.error()" what="maintenance windows" [rows]="4" (retry)="settings.load()">
      @if (draft(); as d) {
        <section class="form" aria-label="Maintenance windows">
          <p class="lede">Limit Dyno's own heavier background work to the hours you choose. It never limits anything you click: a scan or a report you ask for always runs at once. Nothing else on your computer is affected.</p>

          @if (status(); as s) {
            <p class="status" role="status"><span class="dot" [attr.data-tone]="s.enabled && !s.openNow ? 'warn' : 'ok'" aria-hidden="true"></span>{{ statusLine() }}</p>
          }

          <div class="row">
            <div class="lbl"><strong>Only run heavier work in these windows</strong><small>Off means any time.</small></div>
            <mat-slide-toggle [checked]="d.enabled" aria-label="Only run heavier work in maintenance windows" (change)="set({ enabled: $event.checked })" />
          </div>

          @if (d.enabled || d.windows.length > 0) {
            <ul class="windows" aria-label="Windows">
              @for (w of d.windows; track $index; let i = $index) {
                <li class="win" [class.off]="!d.enabled">
                  <pd-day-picker [value]="w.days" [label]="'Days of window ' + (i + 1)" (valueChange)="setDays(i, w, $event)" />
                  <dy-time-field shown="From" [label]="'Window ' + (i + 1) + ' starts'" [value]="time(w.startMinute)" (timeChange)="setTime(i, w, 'startMinute', $event)" />
                  <dy-time-field shown="To" [label]="'Window ' + (i + 1) + ' ends'" [value]="time(w.endMinute)" (timeChange)="setTime(i, w, 'endMinute', $event)" />
                  <span class="sum muted">{{ summary(w) }}</span>
                  <button type="button" matIconButton [attr.aria-label]="'Remove window ' + (i + 1)" matTooltip="Remove window" (click)="remove(i)"><dy-icon name="trash" /></button>
                </li>
              }
            </ul>
            <div class="add">
              <button type="button" matButton="text" [disabled]="d.windows.length >= max" (click)="add()"><dy-icon name="plus" /> Add window</button>
              @if (d.windows.length >= max) { <span class="faint muted">At most {{ max }} windows.</span> }
            </div>
          }

          @if (problem(); as p) {
            <p class="note bad" role="alert"><dy-icon name="alert" /><span>{{ p }}</span></p>
          } @else if (store.maintenanceError(); as e) {
            <p class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></p>
          }

          <div class="foot">
            <button type="button" matButton="outlined" [disabled]="!dirty()" (click)="discard()">Discard changes</button>
            <button type="button" matButton="filled" [disabled]="!dirty() || !!problem() || store.maintenanceSaving()" (click)="save()">{{ store.maintenanceSaving() ? 'Saving…' : 'Save windows' }}</button>
          </div>
        </section>
      }
    </dy-async>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .form { display: grid; gap: var(--sp-3); max-width: 960px; padding: var(--sp-4) var(--sp-5); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
      .lede { margin: 0; color: var(--text-2); }
      .status { margin: 0; display: flex; align-items: center; font-weight: var(--fw-medium); }
      .row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
      .lbl { display: grid; gap: 2px; }
      .lbl small { color: var(--text-3); font-size: var(--fs-sm); }
      .windows { list-style: none; margin: 0; padding: 0; display: grid; }
      .win { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
      .win.off { opacity: 0.55; }
      .sum { flex: 1 1 120px; min-width: 0; font-size: var(--fs-sm); }
      .add { display: flex; align-items: center; gap: var(--sp-3); }
      .foot { display: flex; justify-content: flex-end; gap: var(--sp-2); padding-top: var(--sp-2); border-top: 1px solid var(--border-subtle); }
    `,
  ],
})
export class ExtrasMaintenance implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly settings = inject(SettingsStore);
  protected readonly weekdays = WEEKDAYS;
  protected readonly max = MAX_WINDOWS;
  protected readonly draft = signal<MaintenanceSettings | null>(null);
  protected readonly status = computed(() => this.store.maintenance().data);
  protected readonly statusLine = computed(() => {
    const s = this.status();
    return s ? statusText(s) : '';
  });
  protected readonly dirty = computed(() => {
    const d = this.draft();
    const s = this.settings.settings()?.maintenance;
    return d !== null && s !== undefined && JSON.stringify(d) !== JSON.stringify(s);
  });
  protected readonly problem = computed(() => {
    const d = this.draft();
    return d ? validateMaintenance(d) : null;
  });
  protected readonly time = minuteToTime;
  protected readonly summary = windowText;

  constructor() {
    // A fresh copy whenever the saved settings change (first load, after saving).
    effect(() => {
      const m = this.settings.settings()?.maintenance;
      untracked(() => this.draft.set(m ? structuredClone(m) : null));
    });
  }

  ngOnInit(): void {
    void this.store.ensure('maintenance');
    if (!this.settings.settings()) void this.settings.load();
  }

  protected set(patch: Partial<MaintenanceSettings>): void {
    this.draft.update((d) => {
      if (!d) return d;
      const next = { ...d, ...patch };
      // Turning the windows on with none yet starts with an editable weeknight window.
      return next.enabled && next.windows.length === 0 ? addWindow(next) : next;
    });
  }

  protected setDays(i: number, w: MaintenanceWindow, days: Weekday[]): void {
    this.draft.update((d) => (d ? withWindow(d, i, { ...w, days }) : d));
  }

  protected setTime(i: number, w: MaintenanceWindow, key: 'startMinute' | 'endMinute', text: string): void {
    const minute = timeToMinute(text);
    if (minute !== null) this.draft.update((d) => (d ? withWindow(d, i, { ...w, [key]: minute }) : d));
  }

  protected add(): void {
    this.draft.update((d) => (d ? addWindow(d) : d));
  }

  protected remove(i: number): void {
    this.draft.update((d) => (d ? removeWindow(d, i) : d));
  }

  protected discard(): void {
    const m = this.settings.settings()?.maintenance;
    this.draft.set(m ? structuredClone(m) : null);
  }

  protected async save(): Promise<void> {
    const d = this.draft();
    if (d) await this.store.saveMaintenance(d);
  }
}
