import { CdkContextMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { WhatsNewItem } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { Clipboard } from '../../shared/clipboard';
import { groupByDay } from '../../shared/day-groups';
import { formatBytes } from '../../shared/format';
import { Avatar } from '../../shared/ui/avatar';
import { AgoShortPipe } from '../../shared/ui/ago.pipe';
import { MenuButton, MenuPanel, type MenuEntry } from '../../shared/ui/row-menu';
import { RowItem, RowList } from '../../shared/ui/row-nav';
import { Version } from '../../shared/ui/version';
import { WatchesStore } from '../../state/watches.store';

/**
 * The release feed: newest first, grouped by day. A row shows who, what and when; its actions appear on hover and
 * focus (Open, Download, and a menu); a click opens the watch. Unseen rows carry a dot, seen rows are quieter.
 */
@Component({
  selector: 'sp-whats-new',
  imports: [MatIcon, SuEmptyState, Avatar, Version, AgoShortPipe, MenuButton, MenuPanel, CdkContextMenuTrigger, RowItem, RowList],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (groups().length) {
      <div class="days" spRowList>
      @for (g of groups(); track g.label) {
        <section class="day" [attr.aria-label]="g.label">
          <h3>{{ g.label }}</h3>
          <ul class="list">
            @for (item of g.items; track item.watchId + item.release.version + (item.release.product ?? '')) {
              <li class="item" spRowItem [cdkContextMenuTriggerFor]="ctx" [class.seen]="!item.unseen" (click)="goto(item)" tabindex="-1" (spRowActivate)="goto(item)" (keydown.e)="seen(item)">
                <span class="unseen-dot" [class.off]="!item.unseen" [attr.title]="item.unseen ? 'Not seen yet' : null"></span>
                <sp-avatar [name]="item.watchName" />
                <div class="main">
                  <div class="title truncate">
                    <span class="name">{{ item.watchName }}</span>
                    @if (item.release.prerelease) { <span class="pre">pre-release</span> }
                  </div>
                  <div class="meta truncate">
                    {{ item.release.product ?? item.release.name }}
                    @if (item.release.size !== null) { · {{ size(item.release.size) }} }
                    @if (item.release.artifacts.length > 1) { · {{ item.release.artifacts.length }} files }
                  </div>
                </div>
                <span class="token" [attr.title]="installed(item) ? 'Installed ' + installed(item) + ', latest ' + item.release.version : null">
                  @if (installed(item); as inst) {
                    <sp-ver [value]="inst" /><mat-icon svgIcon="sp:arrow-right" aria-hidden="true" />
                  }
                  <span class="to"><sp-ver [value]="item.release.version" /></span>
                </span>
                <time class="when">{{ item.releasedAtMs | agoShort }}</time>
                <div class="hover-actions" role="presentation" (click)="$event.stopPropagation()" (keydown)="$event.stopPropagation()">
                  <button type="button" class="btn ghost icon sm reveal" (click)="store.openRelease(item.watchId, item.release.version, { product: item.release.product })" [attr.aria-label]="'Open ' + item.release.version + ' of ' + item.watchName" title="Open">
                    <mat-icon svgIcon="sp:external" aria-hidden="true" />
                  </button>
                  <button type="button" class="btn ghost icon sm reveal" (click)="store.download(item.watchId, item.release.version, { product: item.release.product })" [attr.aria-label]="'Download ' + item.release.version + ' of ' + item.watchName" title="Download">
                    <mat-icon svgIcon="sp:download" aria-hidden="true" />
                  </button>
                  <sp-menu-button class="more-wrap" [label]="'More actions for ' + item.watchName + ' ' + item.release.version" [entries]="entries(item)" (picked)="run($event, item)" />
                </div>
                <ng-template #ctx><sp-menu-panel [label]="'Actions for ' + item.watchName + ' ' + item.release.version" [entries]="entries(item)" (picked)="run($event, item)" /></ng-template>
              </li>
            }
          </ul>
        </section>
      }
      </div>
    } @else {
      <pd-empty-state heading="No releases yet" description="New releases show up here as soon as a watch finds one." />
    }
  `,
  styles: `
    :host,
    .days {
      display: grid;
      gap: var(--pd-space-4);
    }
    .day {
      display: grid;
      gap: var(--pd-space-1);
    }
    h3 {
      margin: 0;
      padding: 0 var(--pd-space-3);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-medium);
    }
    .list {
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .item {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      min-height: 56px;
      padding: var(--pd-space-2) var(--pd-space-3);
      border-radius: var(--pd-radius-card);
      cursor: default;
    }
    .item:hover,
    .item:focus-visible {
      background: var(--pd-hover);
    }
    .main {
      flex: 1;
      min-width: 0;
    }
    .title {
      display: flex;
      align-items: baseline;
      gap: var(--pd-space-2);
      font-size: var(--pd-text-body);
    }
    .name {
      font-weight: var(--pd-weight-semibold);
    }
    .pre,
    .meta {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .seen .name {
      font-weight: var(--pd-weight-medium);
      color: var(--pd-text-muted);
    }
    .when {
      min-width: 52px;
      color: var(--pd-text-subtle);
      font-size: var(--pd-text-caption);
      text-align: right;
      white-space: nowrap;
    }
    .hover-actions {
      width: 92px;
      justify-content: flex-end;
    }
  `,
})
export class WhatsNew {
  readonly items = input.required<readonly WhatsNewItem[]>();
  protected readonly store = inject(WatchesStore);
  private readonly router = inject(Router);
  private readonly clipboard = inject(Clipboard);
  private readonly clock = inject(ClockService);

  protected readonly groups = computed(() => groupByDay(this.items(), (i) => i.releasedAtMs, this.clock.now()));

  protected size(bytes: number): string {
    return formatBytes(bytes);
  }

  /** The installed version, only when it differs from the release shown (the arrow token needs both). */
  protected installed(item: WhatsNewItem): string | null {
    const inst = this.store.byId(item.watchId)?.installed ?? null;
    return inst && item.newerThanInstalled ? inst : null;
  }

  protected entries(item: WhatsNewItem): MenuEntry[] {
    return [
      { id: 'details', label: 'Open watch', icon: 'watches' },
      { id: 'open', label: 'Open release', icon: 'external' },
      { id: 'download', label: 'Download', icon: 'download' },
      ...(item.unseen ? [{ id: 'seen', label: 'Mark as seen', icon: 'check', separated: true }] : []),
      { id: 'link', label: 'Copy link', icon: 'link', separated: !item.unseen },
      ...(item.release.sha256 ? [{ id: 'sha', label: 'Copy SHA-256', icon: 'lock' }] : []),
    ];
  }

  protected run(id: string, item: WhatsNewItem): void {
    const target = { product: item.release.product };
    if (id === 'details') this.goto(item);
    else if (id === 'open') void this.store.openRelease(item.watchId, item.release.version, target);
    else if (id === 'download') void this.store.download(item.watchId, item.release.version, target);
    else if (id === 'seen') this.seen(item);
    else if (id === 'link') void this.clipboard.copy(item.release.location, 'Link');
    else if (id === 'sha' && item.release.sha256) void this.clipboard.copy(item.release.sha256, 'Checksum');
  }

  protected seen(item: WhatsNewItem): void {
    if (item.unseen) void this.store.acknowledge(item.watchId, item.release.version);
  }

  protected goto(item: WhatsNewItem): void {
    void this.router.navigate(['/watches', item.watchId]);
  }
}
