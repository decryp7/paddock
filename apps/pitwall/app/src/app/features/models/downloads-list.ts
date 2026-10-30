import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBar } from '@angular/material/progress-bar';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Download } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, bytes, duration, percent } from '../../shared/format';
import { downloadActive, downloadLook } from '../../shared/labels';
import { ModelsStore } from './models.store';
import { SuEmptyState } from '@decryp7/paddock-ui';

/** Time left at the current speed, or null when it can't be estimated. */
export function etaMs(d: Download): number | null {
  if (d.state !== 'downloading' || d.speedBps <= 0) return null;
  return ((d.bytesTotal - d.bytesDone) / d.speedBps) * 1000;
}

/** Model and engine downloads with live progress (pushed), speed, time left, cancel and the resumable state. */
@Component({
  selector: 'pw-downloads-list',
  imports: [MatIconButton, MatProgressBar, MatIconModule, MatTooltipModule, SuEmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.downloads().length === 0) {
      <pd-empty-state heading="No downloads" description="Downloads from Discover and engine installs show their progress here."><mat-icon slot="icon" svgIcon="pw:download" aria-hidden="true" /></pd-empty-state>
    }
    <ul aria-label="Downloads">
      @for (d of live.downloads(); track d.id) {
        <li [attr.data-state]="d.state">
          <span class="ic"><mat-icon [svgIcon]="d.kind === 'engine' ? 'pw:engines' : 'pw:models'" aria-hidden="true" /></span>
          <div class="main">
            <div class="top">
              <span class="name mono">{{ d.name }}</span>
              @if (d.state !== 'downloading') {
                <span class="status" [attr.data-tone]="look(d).tone">{{ look(d).label }}</span>
              }
            </div>
            @if (d.state !== 'done' && d.state !== 'cancelled') {
              <mat-progress-bar mode="determinate" [value]="pct(d)" [attr.data-tone]="d.state === 'failed' ? 'bad' : d.state === 'paused' ? 'warn' : null" [attr.aria-label]="'Progress of ' + d.name" />
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
            <button type="button" matIconButton class="cancel" [disabled]="store.isBusy()('cancel:' + d.id)" [attr.aria-label]="'Cancel ' + d.name" [matTooltip]="d.state === 'paused' ? 'Discard the partial file' : 'Stop the download'" (click)="store.cancelDownload(d.id)">
              <mat-icon svgIcon="pw:close" aria-hidden="true" />
            </button>
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
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) 0;
      border-top: 1px solid var(--pd-hairline);
      &:first-child {
        border-top: none;
        padding-top: 0;
      }
    }
    /* Cancel is a quiet icon on hover or focus, like every row action. */
    .cancel {
      opacity: 0;
    }
    li:hover .cancel,
    li:focus-within .cancel,
    .cancel:focus-visible {
      opacity: 1;
    }
    @media (hover: none) {
      .cancel {
        opacity: 1;
      }
    }
    /* A failed or paused download keeps its bar in the status colour (with the word beside the name). */
    mat-progress-bar[data-tone='bad'] {
      --mat-progress-bar-active-indicator-color: var(--pd-danger);
    }
    mat-progress-bar[data-tone='warn'] {
      --mat-progress-bar-active-indicator-color: var(--pd-warning);
    }
    .ic {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border-radius: var(--pd-radius-control);
      background: var(--pd-hover);
      color: var(--pd-text-muted);
      align-self: start;
    }
    li[data-state='downloading'] .ic {
      background: var(--pd-accent-tint);
      color: var(--pd-accent);
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
      gap: var(--pd-space-3);
    }
    .name {
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      span + span::before {
        content: '·';
        margin-right: var(--pd-space-2);
      }
    }
    .err {
      font-size: var(--pd-text-label);
      color: var(--pd-danger);
      &.warn {
        color: var(--pd-warning);
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
