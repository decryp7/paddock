import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, ElementRef, effect, inject, input, output, signal, viewChild } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { Alarm, ShelveFor } from '../../core/api.types';
import { formatCount } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { AlarmsStore } from '../../state/alarms.store';
import { alarmSuffix, changeLabel, isShelved, needsAcknowledge, severityWord, shortTime, stateTone, stateWord } from './alarms-model';

/** The side panel of one alarm: full values, notes (add one), its change log and the way to the recorder. Esc closes it. */
@Component({
  selector: 'dy-alarm-detail',
  imports: [Icon, RouterLink, CdkMenu, CdkMenuItem, CdkMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <aside class="detail" tabindex="-1" aria-label="Alarm details" (keydown.escape)="closed.emit()">
      <div class="dhead">
        <strong class="ttl">{{ alarm().title }}</strong>
        <button type="button" class="ib" aria-label="Close details" (click)="closed.emit()"><dy-icon name="close" /></button>
      </div>
      <p class="status"><span class="dot" [attr.data-tone]="tone()" aria-hidden="true"></span>{{ state() }}@if (suffix()) {<span class="faint"> · {{ suffix() }}</span>}</p>
      <p class="text">{{ alarm().detail }}</p>
      <dl>
        <dt>Severity</dt><dd>{{ severity() }}</dd>
        <dt>Rule</dt><dd class="id">{{ alarm().rule }}</dd>
        <dt>First seen</dt><dd class="num">{{ time(alarm().firstSeen) }}</dd>
        <dt>Last seen</dt><dd class="num">{{ time(alarm().lastSeen) }}</dd>
        <dt>Occurrences</dt><dd class="num">{{ count(alarm().repeatCount) }}</dd>
        @if (alarm().currentValue !== null) { <dt>Value now</dt><dd class="num">{{ alarm().currentValue }}</dd> }
        @if (alarm().acknowledgedAt !== null) { <dt>Acknowledged</dt><dd class="num">{{ time(alarm().acknowledgedAt!) }}</dd> }
        @if (alarm().clearedAt !== null) { <dt>Returned</dt><dd class="num">{{ time(alarm().clearedAt!) }}</dd> }
      </dl>
      <div class="acts">
        <button type="button" class="btn sm" [disabled]="!ackable()" (click)="store.acknowledge([alarm().id])"><dy-icon name="check" /> Acknowledge</button>
        @if (shelved()) {
          <button type="button" class="btn sm" (click)="store.unshelve(alarm().id)"><dy-icon name="undo" /> Unshelve</button>
        } @else {
          <button type="button" class="btn sm" [cdkMenuTriggerFor]="shelveMenu"><dy-icon name="clock" /> Shelve</button>
        }
        <button type="button" class="btn sm ghost" (click)="store.mute(alarm().rule, !alarm().muted)">{{ alarm().muted ? 'Unmute rule' : 'Mute rule' }}</button>
      </div>

      <h3>Notes</h3>
      @for (n of alarm().notes; track $index) {
        <p class="note"><span class="faint num">{{ time(n.at) }}</span> {{ n.text }}</p>
      } @empty {
        <p class="faint small">No notes yet.</p>
      }
      <form class="add" (submit)="$event.preventDefault(); addNote()">
        <input #noteInput class="input sm" type="text" maxlength="200" aria-label="Add a note" placeholder="Add a note" [value]="draft()" (input)="draft.set($any($event.target).value)" />
        <button type="submit" class="btn sm" [disabled]="!draft().trim()">Add</button>
      </form>

      <h3>History</h3>
      @for (c of store.detailHistory(); track $index) {
        <p class="hist"><span class="faint num">{{ time(c.at) }}</span> {{ label(c) }}</p>
      } @empty {
        <p class="faint small">No changes recorded.</p>
      }
      <a class="btn sm rec" routerLink="/recorder"><dy-icon name="camera" /> Open recorder</a>
    </aside>

    <ng-template #shelveMenu>
      <div class="menu" cdkMenu aria-label="Shelve for">
        @for (o of options; track o.id) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="store.shelve(alarm().id, o.id)">{{ o.label }}</button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: block; }
    .detail { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3) var(--sp-4) var(--sp-4); display: grid; gap: var(--sp-2); align-content: start; }
    .dhead { display: flex; justify-content: space-between; align-items: flex-start; gap: var(--sp-2); }
    .ttl { font-weight: var(--fw-semibold); line-height: 1.35; overflow-wrap: anywhere; }
    .ib { display: grid; place-items: center; width: 26px; height: 26px; flex: none; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover, .ib:focus-visible { background: var(--surface-active); color: var(--text-1); }
    .ib dy-icon { --icon-size: 15px; }
    .status { margin: 0; display: flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-sm); }
    .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; background: var(--text-3); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='calm'] { background: transparent; box-shadow: inset 0 0 0 1.5px var(--text-3); }
    .text { margin: 0; color: var(--text-2); font-size: var(--fs-sm); overflow-wrap: anywhere; }
    dl { margin: 0; display: grid; grid-template-columns: 90px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; user-select: text; }
    .id { font: var(--fw-regular) var(--fs-sm) / 1.3 var(--font-mono); }
    .acts { display: flex; flex-wrap: wrap; gap: var(--sp-2); margin-top: var(--sp-1); }
    h3 { margin: var(--sp-3) 0 0; font: var(--fw-semibold) var(--fs-base) / 1.3 var(--font-sans); }
    .note, .hist { margin: 0; font-size: var(--fs-sm); overflow-wrap: anywhere; }
    .note .faint, .hist .faint { margin-right: var(--sp-2); }
    .small { margin: 0; font-size: var(--fs-sm); }
    .faint { color: var(--text-3); }
    .add { display: flex; gap: var(--sp-2); }
    .add input { flex: 1; }
    .rec { justify-self: start; margin-top: var(--sp-2); text-decoration: none; }
  `,
})
export class AlarmDetail {
  protected readonly store = inject(AlarmsStore);
  readonly alarm = input.required<Alarm>();
  readonly now = input.required<number>();
  /** Bumped by the parent to put the cursor in the note field (from "Add note…"). */
  readonly focusNote = input(0);
  readonly closed = output<void>();
  protected readonly draft = signal('');
  private readonly noteInput = viewChild<ElementRef<HTMLInputElement>>('noteInput');
  protected readonly options: { id: ShelveFor; label: string }[] = [
    { id: 'oneHour', label: '1 hour' },
    { id: 'untilTomorrow', label: 'Until tomorrow' },
    { id: 'untilReboot', label: 'Until restart' },
  ];
  protected readonly count = formatCount;
  protected readonly label = changeLabel;
  protected readonly time = (at: number) => shortTime(at, this.now());
  protected readonly tone = () => stateTone(this.alarm().state);
  protected readonly state = () => stateWord(this.alarm().state);
  protected readonly severity = () => severityWord(this.alarm().severity);
  protected readonly suffix = () => alarmSuffix(this.alarm(), this.now());
  protected readonly shelved = () => isShelved(this.alarm().shelved, this.now());
  protected readonly ackable = () => needsAcknowledge(this.alarm());

  constructor() {
    effect(() => {
      if (this.focusNote() > 0) this.noteInput()?.nativeElement.focus();
    });
  }

  protected async addNote(): Promise<void> {
    const text = this.draft().trim();
    if (text && (await this.store.note(this.alarm().id, text))) this.draft.set('');
  }
}
