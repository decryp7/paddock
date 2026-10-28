import { ChangeDetectionStrategy, Component, ElementRef, input, output, viewChild } from '@angular/core';
import type { CaptureReason, RecorderSnapshotInfo } from '../../core/api.types';
import { formatBytes, formatClock, formatDate } from '../../shared/format';
import { MatTooltip } from '@angular/material/tooltip';

/** What triggered a snapshot, as text: "Manual" or the alarm's title. */
export function reasonText(r: CaptureReason): string {
  return r === 'manual' ? 'Manual' : r.alarm.title;
}

/** Saved snapshots, newest first, as a keyboard-navigable list (arrows move and select, Home/End jump). */
@Component({
  selector: 'dy-snapshot-list',
  imports: [MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="head" aria-hidden="true"><span>Saved</span><span>Reason</span><span class="r">Size</span></div>
    <div #list class="list" role="listbox" tabindex="-1" aria-label="Saved snapshots" (keydown)="onKey($event)">
      @for (s of snapshots(); track s.id) {
        <button type="button" role="option" class="row" [attr.aria-selected]="s.id === selectedId()" [tabindex]="s.id === selectedId() ? 0 : -1" [attr.data-id]="s.id" (click)="picked.emit(s.id)">
          <span class="when">{{ when(s.at) }}</span>
          <span class="why" [matTooltip]="why(s)">{{ why(s) }}</span>
          <span class="size">{{ size(s) }}</span>
        </button>
      }
    </div>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .head, .row { display: grid; grid-template-columns: 112px minmax(0, 1fr) 64px; gap: var(--sp-3); align-items: center; padding: 0 var(--sp-3); }
    .head { height: 30px; border-bottom: 1px solid var(--border); color: var(--text-3); font-size: var(--fs-xs); position: sticky; top: 0; background: var(--surface-1); z-index: 3; }
    .r, .size { text-align: right; }
    .row { width: 100%; height: 40px; border: 0; border-bottom: 1px solid var(--border-subtle); background: transparent; color: var(--text-1); font: inherit; font-size: var(--fs-base); text-align: left; cursor: default; position: relative; }
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
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');

  protected readonly why = (s: RecorderSnapshotInfo) => reasonText(s.reason);
  protected readonly size = (s: RecorderSnapshotInfo) => formatBytes(s.bytes);
  protected when(at: number): string {
    return `${formatDate(at, Date.now())}, ${formatClock(at)}`;
  }

  protected onKey(e: KeyboardEvent): void {
    const rows = this.snapshots();
    if (rows.length === 0) return;
    const at = Math.max(0, rows.findIndex((s) => s.id === this.selectedId()));
    const next = e.key === 'ArrowDown' ? at + 1 : e.key === 'ArrowUp' ? at - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1 : null;
    if (next === null) return;
    e.preventDefault();
    const target = rows[Math.min(rows.length - 1, Math.max(0, next))]!;
    this.picked.emit(target.id);
    queueMicrotask(() => this.list().nativeElement.querySelector<HTMLElement>(`[data-id="${target.id}"]`)?.focus());
  }
}
