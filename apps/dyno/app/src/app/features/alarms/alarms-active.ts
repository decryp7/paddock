import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import type { Alarm, ShelveFor } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { AlarmsStore } from '../../state/alarms.store';
import { alarmSuffix, isShelved, needsAcknowledge, severityWord, shortTime, stateTone, stateWord } from './alarms-model';
import { AlarmDetail } from './alarm-detail';

const SEVERITY_ORDER: Record<Alarm['severity'], number> = { critical: 0, warning: 1, info: 2 };

/**
 * The open alarms as one table (newest first) with a detail panel beside it. The section action acknowledges everything that is
 * waiting; each row offers acknowledge and shelve on hover or focus, and the same actions in its context menu.
 */
@Component({
  selector: 'dy-alarms-active',
  imports: [AsyncState, AlarmDetail, PdTable, PdCell],
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
        <pd-table
          label="Open alarms"
          [rows]="store.alarms()"
          [columns]="columns"
          [rowKey]="rowKey"
          [filterable]="false"
          [rowActions]="actions"
          [maxHeight]="560"
          [selectedKey]="selectedKey()"
          (rowActivate)="pick($event)"
          (action)="run($event)"
          (keydown.escape)="escape($event)"
        >
          <ng-template pdCell="unread" let-a>@if (unread(a)) {<span class="unread" role="img" aria-label="Not acknowledged"></span>}</ng-template>
          <ng-template pdCell="severity" let-a><span class="dot" [attr.data-tone]="a.severity === 'critical' ? 'bad' : a.severity === 'warning' ? 'warn' : 'info'" [attr.aria-label]="sev(a)" role="img"></span><span [class.muted]="a.severity === 'info'">{{ sev(a) }}</span></ng-template>
          <ng-template pdCell="title" let-a>
            <span class="line"><span class="t">{{ a.title }}</span>@if (suffix(a); as s) {<span class="sfx">{{ s }}</span>}</span>
          </ng-template>
          <ng-template pdCell="rule" let-a><span class="id muted">{{ a.rule }}</span></ng-template>
          <ng-template pdCell="state" let-a><span class="dot" [attr.data-tone]="tone(a)" aria-hidden="true"></span><span class="ell" [class.muted]="tone(a) === 'calm'">{{ state(a) }}</span></ng-template>
        </pd-table>
        @if (store.selected(); as sel) {
          <dy-alarm-detail [alarm]="sel" [now]="now()" [focusNote]="focusNote()" (closed)="closeDetail()" />
        }
      </div>
    </dy-async>
  `,
  styles: `
    :host { display: block; }
    dy-async { display: grid; gap: var(--sp-3); }
    .section-head { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); min-height: var(--control-h); margin-bottom: var(--sp-3); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 320px; }
    .unread { display: block; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
    .line { display: flex; gap: var(--sp-2); align-items: baseline; min-width: 0; }
    .t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--fw-medium); }
    .sfx { flex: none; color: var(--text-3); font-size: var(--fs-xs); }
    .muted { color: var(--text-2); }
    .ell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .id { font: var(--fw-regular) var(--fs-sm) / 1.2 var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; margin-inline-end: var(--sp-2); background: var(--text-3); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='info'] { background: var(--info); }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='calm'] { background: transparent; box-shadow: inset 0 0 0 1.5px var(--text-3); }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class AlarmsActive {
  protected readonly store = inject(AlarmsStore);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly now = signal(Date.now());
  protected readonly busy = signal(false);
  protected readonly focusNote = signal(0);
  /** The shelve choices, in the row's menu ("Shelve for 1 hour"). */
  private readonly shelveOptions: { id: ShelveFor; label: string }[] = [
    { id: 'oneHour', label: '1 hour' },
    { id: 'untilTomorrow', label: 'until tomorrow' },
    { id: 'untilReboot', label: 'until restart' },
  ];
  protected readonly rowKey = (a: Alarm): string => String(a.id);
  protected readonly selectedKey = computed(() => (this.store.selectedId() === null ? null : String(this.store.selectedId())));
  protected readonly columns: PdColumn<Alarm>[] = [
    { key: 'unread', header: 'Not acknowledged', value: (a) => (needsAcknowledge(a) ? 0 : 1), text: () => '', width: '14px', headerHidden: true },
    { key: 'severity', header: 'Severity', value: (a) => SEVERITY_ORDER[a.severity], text: (a) => severityWord(a.severity), width: '92px' },
    { key: 'title', header: 'Alarm', value: (a) => a.title, required: true, width: 'minmax(0, 2fr)' },
    { key: 'detail', header: 'Detail', value: (a) => a.detail, width: 'minmax(0, 2fr)', hideBelow: 900 },
    { key: 'rule', header: 'Rule', value: (a) => a.rule, width: 'minmax(0, 1fr)', hideBelow: 1000 },
    { key: 'state', header: 'State', value: (a) => stateWord(a.state), width: '190px' },
    { key: 'firstSeen', header: 'First seen', value: (a) => a.firstSeen, text: (a) => this.time(a.firstSeen), width: '84px', hideBelow: 1000 },
    { key: 'lastSeen', header: 'Last seen', value: (a) => a.lastSeen, text: (a) => this.time(a.lastSeen), width: '84px' },
    { key: 'count', header: 'Count', value: (a) => a.repeatCount, text: (a) => `×${String(a.repeatCount)}`, width: '64px', align: 'end' },
  ];
  protected readonly sev = (a: Alarm) => severityWord(a.severity);
  protected readonly state = (a: Alarm) => stateWord(a.state);
  protected readonly tone = (a: Alarm) => stateTone(a.state);
  protected readonly unread = needsAcknowledge;
  protected readonly suffix = (a: Alarm) => alarmSuffix(a, this.now());
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
    queueMicrotask(() => this.host.querySelector<HTMLElement>(`[role="row"][data-key="${String(id)}"]`)?.focus());
  }

  protected async ackAll(): Promise<void> {
    this.busy.set(true);
    await this.store.acknowledgeAll();
    this.busy.set(false);
  }

  protected ack(a: Alarm): void {
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

  /** The row actions: acknowledge and a one-hour shelve on hover, the rest (every shelve choice included) in the menu. */
  protected readonly actions = (a: Alarm): PdRowAction[] => [
    { id: 'ack', label: 'Acknowledge', icon: 'check', inline: true, disabled: !needsAcknowledge(a) },
    { id: 'shelve:oneHour', label: 'Shelve for 1 hour', icon: 'clock', inline: true },
    ...this.shelveOptions.slice(1).map((o): PdRowAction => ({ id: `shelve:${o.id}`, label: `Shelve ${o.label}`, icon: 'clock' })),
    ...(isShelved(a.shelved, this.now()) ? [{ id: 'unshelve', label: 'Unshelve', icon: 'clock' }] : []),
    { id: 'note', label: 'Add note…', icon: 'edit' },
    { id: 'mute', label: a.muted ? 'Unmute this rule' : 'Mute this rule', icon: 'bell' },
    { id: 'history', label: 'Show history', icon: 'history' },
  ];

  protected run({ id, row }: PdRowActionEvent<Alarm>): void {
    if (id === 'ack') this.ack(row);
    else if (id.startsWith('shelve:')) this.shelve(row, id.slice(7) as ShelveFor);
    else if (id === 'unshelve') void this.store.unshelve(row.id);
    else if (id === 'note') this.addNote(row);
    else if (id === 'mute') void this.store.mute(row.rule, !row.muted);
    else this.showHistory(row);
  }

  /** Escape in the table closes the detail panel and puts the focus back on its row. */
  protected escape(e: Event): void {
    if (this.store.selectedId() === null) return;
    e.preventDefault();
    this.closeDetail();
  }
}
