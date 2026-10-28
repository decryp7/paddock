import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, DestroyRef, inject, signal } from '@angular/core';
import type { Alarm, ShelveFor } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { AlarmsStore } from '../../state/alarms.store';
import { alarmSuffix, isShelved, needsAcknowledge, severityWord, shortTime, stateTone, stateWord } from './alarms-model';
import { TABLE_CSS } from './alarms-css';
import { AlarmDetail } from './alarm-detail';
import { MatButton } from '@angular/material/button';

/**
 * The open alarms as one table (newest first) with a detail panel beside it. The section action acknowledges everything that is
 * waiting; each row offers acknowledge and shelve on hover or focus, and the same actions in its context menu.
 */
@Component({
  selector: 'dy-alarms-active',
  imports: [MatButton, AsyncState, Icon, AlarmDetail, CdkMenu, CdkMenuItem, CdkMenuTrigger, CdkContextMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="store.loaded()" [error]="store.error()" [empty]="store.alarms().length === 0" emptyTitle="No open alarms" emptyText="When a rule fires, its alarm waits here until you acknowledge it." emptyIcon="bell" what="alarms" [rows]="4" (retry)="store.load()">
      <div class="section-head">
        <p class="lead num">{{ store.alarms().length }} open · {{ store.unacknowledged().length }} not acknowledged</p>
        @if (store.unacknowledged().length > 0) {
          <button type="button" matButton="filled" [disabled]="busy()" (click)="ackAll()">Acknowledge all ({{ store.unacknowledged().length }})</button>
        }
      </div>
      <div class="layout" [class.with-detail]="!!store.selected()">
        <div class="grid" role="grid" tabindex="-1" aria-label="Open alarms" [attr.aria-rowcount]="store.alarms().length + 1" (keydown)="key($event)">
          <div class="head" role="row">
            <span role="columnheader"><span class="visually-hidden">Unread</span></span>
            <span role="columnheader">Severity</span>
            <span role="columnheader">Alarm</span>
            <span role="columnheader" class="c-rule">Rule</span>
            <span role="columnheader">State</span>
            <span role="columnheader" class="c-first">First seen</span>
            <span role="columnheader">Last seen</span>
            <span role="columnheader" class="r">Count</span>
            <span role="columnheader"><span class="visually-hidden">Actions</span></span>
          </div>
          @for (a of store.alarms(); track a.id) {
            <div
              class="row"
              role="row"
              tabindex="0"
              [attr.data-id]="a.id"
              [class.selected]="store.selectedId() === a.id"
              [attr.aria-selected]="store.selectedId() === a.id"
              [cdkContextMenuTriggerFor]="menu"
              [cdkContextMenuTriggerData]="{ alarm: a }"
              (click)="pick(a)"
              (keydown.enter)="pick(a)"
              (keydown.space)="$event.preventDefault(); pick(a)"
            >
              <span class="cell" role="gridcell">@if (unread(a)) {<span class="unread" title="Not acknowledged" aria-label="Not acknowledged"></span>}</span>
              <span class="cell" role="gridcell"><span class="dot" [attr.data-tone]="a.severity === 'critical' ? 'bad' : a.severity === 'warning' ? 'warn' : 'info'" [attr.aria-label]="sev(a)" role="img"></span>@if (a.severity === 'critical') {<span>Critical</span>}</span>
              <span class="cell title-cell" role="gridcell">
                <span class="two">
                  <span class="line"><span class="t truncate" [title]="a.title">{{ a.title }}</span>@if (suffix(a); as s) {<span class="sfx">{{ s }}</span>}</span>
                  <span class="d truncate" [title]="a.detail">{{ a.detail }}</span>
                </span>
              </span>
              <span class="cell c-rule muted" role="gridcell"><span class="truncate id" [title]="a.rule">{{ a.rule }}</span></span>
              <span class="cell" role="gridcell"><span class="dot" [attr.data-tone]="tone(a)" aria-hidden="true"></span><span [class.muted]="tone(a) === 'calm'">{{ state(a) }}</span></span>
              <span class="cell c-first muted num" role="gridcell">{{ time(a.firstSeen) }}</span>
              <span class="cell muted num" role="gridcell">{{ time(a.lastSeen) }}</span>
              <span class="cell r muted num" role="gridcell">×{{ a.repeatCount }}</span>
              <span class="cell acts" role="gridcell">
                <button type="button" class="ib" [attr.aria-label]="'Acknowledge ' + a.title" title="Acknowledge" [disabled]="!ackable(a)" (click)="ack(a, $event)"><dy-icon name="check" /></button>
                <button type="button" class="ib" [attr.aria-label]="'Shelve ' + a.title" title="Shelve" [cdkMenuTriggerFor]="shelveMenu" [cdkMenuTriggerData]="{ alarm: a }" (click)="$event.stopPropagation()"><dy-icon name="clock" /></button>
                <button type="button" class="ib more" [attr.aria-label]="'More actions for ' + a.title" [cdkMenuTriggerFor]="menu" [cdkMenuTriggerData]="{ alarm: a }" (click)="$event.stopPropagation()"><dy-icon name="more" /></button>
              </span>
            </div>
          }
        </div>
        @if (store.selected(); as sel) {
          <dy-alarm-detail [alarm]="sel" [now]="now()" [focusNote]="focusNote()" (closed)="closeDetail()" />
        }
      </div>
    </dy-async>

    <ng-template #menu let-alarm="alarm">
      <div class="menu" cdkMenu [attr.aria-label]="'Actions for ' + alarm.title">
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!ackable(alarm)" (cdkMenuItemTriggered)="ack(alarm)"><dy-icon name="check" /> Acknowledge</button>
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuTriggerFor]="shelveMenu" [cdkMenuTriggerData]="{ alarm }"><dy-icon name="clock" /> Shelve… <dy-icon name="chevron-right" class="trail" /></button>
        @if (shelved(alarm)) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="store.unshelve(alarm.id)"><dy-icon name="undo" /> Unshelve</button>
        }
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="addNote(alarm)"><dy-icon name="edit" /> Add note…</button>
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="store.mute(alarm.rule, !alarm.muted)"><dy-icon name="bell" /> {{ alarm.muted ? 'Unmute this rule' : 'Mute this rule' }}</button>
        <div class="menu-sep"></div>
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="showHistory(alarm)"><dy-icon name="list" /> Show history</button>
      </div>
    </ng-template>
    <ng-template #shelveMenu let-alarm="alarm">
      <div class="menu" cdkMenu aria-label="Shelve for">
        @for (o of shelveOptions; track o.id) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="shelve(alarm, o.id)">{{ o.label }}</button>
        }
      </div>
    </ng-template>
  `,
  styles: [
    TABLE_CSS,
    `
    :host { display: block; }
    dy-async { display: grid; gap: var(--sp-3); }
    .section-head { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); min-height: var(--control-h); margin-bottom: var(--sp-3); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 320px; }
    .head, .row { --cols: 14px 64px minmax(280px, 2.4fr) minmax(120px, 1fr) 190px 80px 80px 52px 40px; }
    .row.selected { background: var(--accent-soft); box-shadow: inset 2px 0 0 var(--accent); }
    .unread { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
    .title-cell { padding: 6px 0; }
    .two { display: grid; gap: 1px; min-width: 0; flex: 1; }
    .line { display: flex; gap: var(--sp-2); align-items: baseline; min-width: 0; }
    .t { font-size: var(--fs-base); font-weight: var(--fw-medium); }
    .sfx { flex: none; color: var(--text-3); font-size: var(--fs-xs); }
    .d { color: var(--text-3); font-size: var(--fs-xs); }
    @container (max-width: 1000px) {
      .c-rule, .c-first { display: none; }
      .head, .row { --cols: 14px 84px minmax(160px, 1fr) 180px 96px 52px 88px; }
    }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
  ],
})
export class AlarmsActive {
  protected readonly store = inject(AlarmsStore);
  protected readonly now = signal(Date.now());
  protected readonly busy = signal(false);
  protected readonly focusNote = signal(0);
  protected readonly shelveOptions: { id: ShelveFor; label: string }[] = [
    { id: 'oneHour', label: '1 hour' },
    { id: 'untilTomorrow', label: 'Until tomorrow' },
    { id: 'untilReboot', label: 'Until restart' },
  ];
  protected readonly sev = (a: Alarm) => severityWord(a.severity);
  protected readonly state = (a: Alarm) => stateWord(a.state);
  protected readonly tone = (a: Alarm) => stateTone(a.state);
  protected readonly ackable = needsAcknowledge;
  protected readonly unread = needsAcknowledge;
  protected readonly suffix = (a: Alarm) => alarmSuffix(a, this.now());
  protected readonly shelved = (a: Alarm) => isShelved(a.shelved, this.now());
  protected readonly time = (at: number) => shortTime(at, this.now());

  constructor() {
    const timer = setInterval(() => this.now.set(Date.now()), 30_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  protected pick(a: Alarm): void {
    void this.store.select(a.id);
  }

  protected closeDetail(): void {
    const id = this.store.selectedId();
    void this.store.select(null);
    queueMicrotask(() => document.querySelector<HTMLElement>(`.row[data-id="${String(id)}"]`)?.focus());
  }

  protected async ackAll(): Promise<void> {
    this.busy.set(true);
    await this.store.acknowledgeAll();
    this.busy.set(false);
  }

  protected ack(a: Alarm, e?: Event): void {
    e?.stopPropagation();
    void this.store.acknowledge([a.id]);
  }

  protected shelve(a: Alarm, until: ShelveFor): void {
    void this.store.shelve(a.id, until);
  }

  protected addNote(a: Alarm): void {
    void this.store.select(a.id);
    this.focusNote.update((n) => n + 1);
  }

  protected showHistory(a: Alarm): void {
    void this.store.select(a.id);
  }

  protected key(e: KeyboardEvent): void {
    const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('.row')];
    const i = rows.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown') rows[Math.min(rows.length - 1, i + 1)]?.focus();
    else if (e.key === 'ArrowUp') rows[Math.max(0, i - 1)]?.focus();
    else if (e.key === 'Escape' && this.store.selectedId() !== null) this.closeDetail();
    else if ((e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) && i >= 0) rows[i]!.querySelector<HTMLElement>('.more')?.click();
    else return;
    e.preventDefault();
  }
}
