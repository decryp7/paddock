import { CdkListbox, CdkOption } from '@angular/cdk/listbox';
import { ChangeDetectionStrategy, ChangeDetectorRef, Component, afterRenderEffect, inject, input, output, viewChild } from '@angular/core';
import type { CaptureReason, RecorderSnapshotInfo } from '../../core/api.types';
import { formatStorage, formatClock, formatDate } from '../../shared/format';
import { FullTextTip } from '../../shared/full-text-tip';

/** What triggered a snapshot, as text: "Manual" or the alarm's title. */
export function reasonText(r: CaptureReason): string {
  return r === 'manual' ? 'Manual' : r.alarm.title;
}

/** Saved snapshots, newest first, as a CDK listbox (arrows move and select, Home/End jump, typeahead). */
@Component({
  selector: 'dy-snapshot-list',
  imports: [CdkListbox, CdkOption, FullTextTip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="head" aria-hidden="true"><span>Saved</span><span>Reason</span><span class="r">Size</span></div>
    <div cdkListbox class="list" aria-label="Saved snapshots" [cdkListboxNavigationWrapDisabled]="true" (cdkListboxValueChange)="onValue($event.value)">
      @for (s of snapshots(); track s.id) {
        <div [cdkOption]="s.id" class="row" [attr.data-id]="s.id" (focus)="pick(s.id)">
          <span class="when">{{ when(s.at) }}</span>
          <span class="why" [dyFullText]="why(s)">{{ why(s) }}</span>
          <span class="size">{{ size(s) }}</span>
        </div>
      }
    </div>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .head, .row { display: grid; grid-template-columns: 112px minmax(0, 1fr) 64px; gap: var(--sp-3); align-items: center; padding: 0 var(--sp-3); }
    .head { height: 30px; border-bottom: 1px solid var(--border); color: var(--text-3); font-size: var(--fs-xs); position: sticky; top: 0; background: var(--surface-1); z-index: 3; }
    .r, .size { text-align: right; }
    .row { width: 100%; height: 40px; border-bottom: 1px solid var(--border-subtle); background: transparent; color: var(--text-1); font-size: var(--fs-base); text-align: left; cursor: default; position: relative; box-sizing: border-box; outline: none; }
    .row:hover { background: var(--surface-hover); }
    .row[aria-selected='true'] { background: var(--accent-soft); }
    .row[aria-selected='true']::before { content: ''; position: absolute; left: 0; top: 6px; bottom: 6px; width: 3px; border-radius: 2px; background: var(--accent); }
    .row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    .when, .size { white-space: nowrap; font-variant-numeric: tabular-nums; }
    .size { color: var(--text-2); }
    .why { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-2); }
  `,
})
export class SnapshotList {
  readonly snapshots = input.required<readonly RecorderSnapshotInfo[]>();
  readonly selectedId = input<number | null>(null);
  readonly picked = output<number>();
  private readonly listbox = viewChild(CdkListbox);
  private readonly changes = inject(ChangeDetectorRef);

  constructor() {
    // The listbox takes its value after the options are drawn (a value it cannot find among its options reads as invalid), and the
    // options' aria-selected is drawn again after it.
    afterRenderEffect(() => {
      const id = this.selectedId();
      const box = this.listbox();
      if (!box) return;
      box.value = id !== null && this.snapshots().some((s) => s.id === id) ? [id] : [];
      this.changes.markForCheck();
    });
  }

  protected readonly why = (s: RecorderSnapshotInfo) => reasonText(s.reason);
  protected readonly size = (s: RecorderSnapshotInfo) => formatStorage(s.bytes);
  protected when(at: number): string {
    return `${formatDate(at, Date.now())}, ${formatClock(at)}`;
  }

  /** The listbox selected an option (click, Enter or Space). */
  protected onValue(value: readonly unknown[]): void {
    const id = value[0];
    if (typeof id === 'number') this.pick(id);
  }

  /** Selection follows the focus: moving with the arrows picks the snapshot, and picking the one already shown does nothing. */
  protected pick(id: number): void {
    if (id !== this.selectedId()) this.picked.emit(id);
  }
}
