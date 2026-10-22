import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Download } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, bytes, duration, percent } from '../../shared/format';
import { downloadActive, downloadLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { ModelsStore } from './models.store';

/** Time left at the current speed, or null when it can't be estimated. */
export function etaMs(d: Download): number | null {
  if (d.state !== 'downloading' || d.speedBps <= 0) return null;
  return ((d.bytesTotal - d.bytesDone) / d.speedBps) * 1000;
}

/** Model and engine downloads with live progress (pushed), speed, time left, cancel and the resumable state. */
@Component({
  selector: 'pw-downloads-list',
  imports: [MatIconModule, MatTooltipModule, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.downloads().length === 0) {
      <pw-empty-state icon="download" heading="No downloads" text="Downloads from Discover and engine installs show their progress here." />
    }
    <ul aria-label="Downloads">
      @for (d of live.downloads(); track d.id) {
        <li [attr.data-state]="d.state">
          <span class="ic"><mat-icon [svgIcon]="d.kind === 'engine' ? 'pw:engines' : 'pw:models'" aria-hidden="true" /></span>
          <div class="main">
            <div class="top">
              <span class="name mono">{{ d.name }}</span>
              <span class="pill dot" [attr.data-tone]="look(d).tone">{{ look(d).label }}</span>
            </div>
            @if (d.state !== 'done' && d.state !== 'cancelled') {
              <div class="meter" [attr.data-tone]="d.state === 'failed' ? 'bad' : d.state === 'paused' ? 'warn' : null" role="progressbar" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="pct(d)" [attr.aria-label]="'Progress of ' + d.name">
                <span [style.width.%]="pct(d)"></span>
              </div>
            }
            <div class="meta num">
              <span>{{ size(d.bytesDone) }} of {{ size(d.bytesTotal) }}</span>
              @if (d.state === 'downloading') {
                <span>{{ size(d.speedBps) }}/s</span>
                @if (eta(d); as left) {
                  <span>{{ dur(left) }} left</span>
                }
              }
              <span>from {{ d.host }}</span>
              <span>started {{ when(d.startedAt, now()) }}</span>
            </div>
            @if (d.error) {
              <p class="err" [class.warn]="d.state === 'paused'">{{ d.error }}</p>
            }
          </div>
          @if (downloadActive(d.state) || d.state === 'paused') {
            <button type="button" class="btn sm" [disabled]="store.isBusy()('cancel:' + d.id)" [attr.aria-label]="'Cancel ' + d.name" [matTooltip]="d.state === 'paused' ? 'Discard the partial file' : 'Stop the download'" (click)="store.cancelDownload(d.id)">Cancel</button>
          }
        </li>
      }
    </ul>
  `,
  styles: `
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
    }
    li {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto;
      align-items: center;
      gap: var(--sp-3);
      padding: var(--sp-3) 0;
      border-top: 1px solid var(--border-soft);
      &:first-child {
        border-top: none;
        padding-top: 0;
      }
    }
    .ic {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border-radius: var(--r-md);
      background: var(--bg-hover);
      color: var(--text-2);
      align-self: start;
    }
    li[data-state='downloading'] .ic {
      background: var(--accent-soft);
      color: var(--accent);
    }
    .main {
      display: grid;
      gap: 6px;
      min-width: 0;
    }
    .top {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-3);
    }
    .name {
      font-weight: var(--fw-medium);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-2);
      font-size: var(--fs-xs);
      color: var(--text-3);
      span + span::before {
        content: '·';
        margin-right: var(--sp-2);
      }
    }
    .err {
      font-size: var(--fs-sm);
      color: var(--bad);
      &.warn {
        color: var(--warn);
      }
    }
  `,
})
export class DownloadsList {
  readonly now = input.required<number>();
  protected readonly live = inject(LiveStore);
  protected readonly store = inject(ModelsStore);
  protected readonly look = (d: Download) => downloadLook(d.state);
  protected readonly downloadActive = downloadActive;
  protected readonly size = bytes;
  protected readonly dur = duration;
  protected readonly when = ago;
  protected readonly eta = etaMs;
  protected pct(d: Download): number {
    return percent(d.bytesDone, d.bytesTotal);
  }
}
