import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { WhatsNewItem } from '../../core/api.types';
import { formatBytes } from '../../shared/format';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { KindIcon } from '../../shared/ui/kind-icon';
import { WatchesStore } from '../../state/watches.store';

/** The newest releases across all watches, with the actions you want on them. */
@Component({
  selector: 'sp-whats-new',
  imports: [MatIcon, RouterLink, SuEmptyState, KindIcon, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (items().length) {
      <ul class="list">
        @for (item of items(); track item.watchId + item.release.version) {
          <li class="item" [class.unseen]="item.unseen">
            <sp-kind-icon [kind]="item.sourceKind" />
            <div class="main">
              <div class="title">
                <span class="ver">{{ item.release.version }}</span>
                @if (item.unseen) {
                  <span class="chip accent">New</span>
                }
                @if (item.newerThanInstalled) {
                  <span class="chip info">Newer than installed</span>
                }
                @if (item.release.prerelease) {
                  <span class="chip">Pre-release</span>
                }
              </div>
              <div class="meta truncate">
                <a [routerLink]="['/watches', item.watchId]">{{ item.watchName }}</a>
                · released {{ item.releasedAtMs | ago }}
                @if (item.release.size !== null) {
                  · {{ size(item.release.size) }}
                }
              </div>
            </div>
            <div class="row-actions">
              <button type="button" class="btn sm" (click)="store.openRelease(item.watchId, item.release.version)" [attr.aria-label]="'Open ' + item.release.version + ' of ' + item.watchName">
                <mat-icon svgIcon="sp:external" aria-hidden="true" />Open
              </button>
              <button type="button" class="btn sm" (click)="store.download(item.watchId, item.release.version)" [attr.aria-label]="'Download ' + item.release.version + ' of ' + item.watchName">
                <mat-icon svgIcon="sp:download" aria-hidden="true" />Download
              </button>
              @if (item.unseen) {
                <button type="button" class="btn sm primary" (click)="store.acknowledge(item.watchId, item.release.version)" [attr.aria-label]="'Mark ' + item.release.version + ' of ' + item.watchName + ' as seen'">
                  <mat-icon svgIcon="sp:check" aria-hidden="true" />Mark seen
                </button>
              }
            </div>
          </li>
        }
      </ul>
    } @else {
      <pd-empty-state heading="No releases yet" description="New releases show up here as soon as a watch finds one." />
    }
  `,
  styles: `
    .list {
      margin: 0;
      padding: 0;
      list-style: none;
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-1);
    }
    .item {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .item:last-child {
      border-bottom: 0;
    }
    .main {
      flex: 1;
      min-width: 0;
    }
    .title {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
    }
    .ver {
      font-size: var(--pd-text-subtitle);
    }
    .unseen .ver {
      color: var(--pd-accent);
    }
    .meta {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
  `,
})
export class WhatsNew {
  readonly items = input.required<readonly WhatsNewItem[]>();
  protected readonly store = inject(WatchesStore);

  protected size(bytes: number): string {
    return formatBytes(bytes);
  }
}
