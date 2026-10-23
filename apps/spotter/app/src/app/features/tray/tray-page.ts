import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { BACKEND } from '../../core/backend';
import { platformFromUserAgent } from '../../core/tauri-backend';
import { observeHeight } from '../../shared/observe-height';
import { AgoShortPipe } from '../../shared/ui/ago.pipe';
import { Avatar } from '../../shared/ui/avatar';
import { Version } from '../../shared/ui/version';
import { WatchesStore } from '../../state/watches.store';

/** The tray popup: unseen releases at a glance. Frameless and transparent; the shell draws the OS material behind. */
@Component({
  selector: 'sp-tray-page',
  imports: [MatIcon, SuTrayPopover, AgoShortPipe, Avatar, Version],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Spotter" [platform]="platform" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/spotter-small.svg" alt="" width="20" height="20" />
        <strong>Spotter</strong>
        @if (unseen().length) {
          <span class="pill" role="status">{{ store.unseen().length }} new</span>
        }
      </div>
      @if (store.error(); as err) {
        <p class="error" role="alert">{{ err.message }} <button type="button" class="btn sm" (click)="store.load()">Retry</button></p>
      } @else if (!store.loaded()) {
        <p class="muted" role="status">Loading…</p>
      } @else if (unseen().length) {
        <ul class="list">
          @for (w of unseen(); track w.id) {
            <li>
              <button type="button" class="item" (click)="open(w.id)" [attr.aria-label]="w.name + ' ' + w.latest?.version + ', open'">
                <sp-avatar [name]="w.name" [watch]="w.id" [size]="32" />
                <span class="main">
                  <span class="name truncate">{{ w.name }}</span>
                  <span class="meta truncate">
                    @if (w.newerThanInstalled && w.installed && w.latest) {
                      <sp-ver [value]="w.installed" /> → <sp-ver [value]="w.latest.version" />
                    } @else if (w.latest) {
                      <sp-ver [value]="w.latest.version" />
                    }
                    · {{ w.latest?.firstSeenMs | agoShort }}
                  </span>
                </span>
              </button>
              <button type="button" class="btn ghost icon sm check" (click)="store.acknowledge(w.id, null)" [attr.aria-label]="'Mark ' + w.name + ' as seen'" title="Mark as seen"><mat-icon svgIcon="sp:check" aria-hidden="true" /></button>
            </li>
          }
        </ul>
        @if (hidden() > 0) {
          <p class="more muted">+{{ hidden() }} more in Spotter</p>
        }
      } @else {
        <div class="empty">
          <mat-icon svgIcon="sp:check-all" aria-hidden="true" />
          <strong>All caught up</strong>
          <span class="muted">You have seen every release.</span>
        </div>
      }
      <div slot="footer" class="foot">
        @if (unseen().length > 1) {
          <button type="button" class="btn ghost sm" (click)="store.acknowledgeAll()">Mark all seen</button>
        }
        <span class="spacer"></span>
        <button type="button" class="btn primary sm" (click)="openMain()">Open Spotter</button>
      </div>
    </pd-tray-popover>
  `,
  styles: `
    :host {
      display: block;
    }
    .more {
      margin: 0;
      padding: var(--pd-space-2) var(--pd-space-4);
      font-size: var(--pd-text-caption);
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      width: 100%;
    }
    .pill {
      margin-left: auto;
      padding: 1px var(--pd-space-2);
      border-radius: var(--pd-radius-pill);
      background: var(--pd-surface-3);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
    }
    .empty {
      display: grid;
      justify-items: center;
      gap: var(--pd-space-1);
      padding: var(--pd-space-6) var(--pd-space-4);
      text-align: center;
    }
    .empty mat-icon {
      width: 32px;
      height: 32px;
      color: var(--pd-text-subtle);
    }
    li:not(:hover):not(:focus-within) .check {
      opacity: 0;
    }
    .list {
      display: grid;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    li {
      display: flex;
      align-items: center;
      gap: var(--pd-space-1);
    }
    .item {
      display: flex;
      flex: 1;
      align-items: center;
      gap: var(--pd-space-2);
      min-width: 0;
      padding: var(--pd-space-2);
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    .item:hover {
      background: var(--pd-hover);
    }
    .main {
      display: grid;
      flex: 1;
      min-width: 0;
    }
    .name {
      font-weight: var(--pd-weight-medium);
    }
    .meta,
    .muted {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .error {
      color: var(--pd-danger);
    }
    .foot {
      display: flex;
      align-items: center;
      width: 100%;
      gap: var(--pd-space-2);
    }
    .spacer {
      flex: 1;
    }
  `,
})
export class TrayPage {
  protected readonly store = inject(WatchesStore);
  private readonly backend = inject(BACKEND);
  protected readonly unseen = computed(() => this.store.unseen().slice(0, 5));
  /** New releases beyond the rows shown. */
  protected readonly hidden = computed(() => Math.max(0, this.store.unseen().length - this.unseen().length));
  protected readonly platform: SuTrayPlatform = (() => {
    const p = platformFromUserAgent(navigator.userAgent);
    return p === 'windows' ? 'windows' : p === 'linux' ? 'linux' : 'macos';
  })();

  constructor() {
    this.store.start();
    // The window is as tall as the popup's content (no empty gap under a short list); the shell clamps the height.
    const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    const destroy = inject(DestroyRef);
    afterNextRender(() => {
      const popup = host.querySelector('pd-tray-popover');
      if (popup) destroy.onDestroy(observeHeight(popup, (h) => void this.backend.fitTray(h).catch(() => undefined)));
    });
  }

  protected close(): void {
    void this.backend.hideTray().catch(() => undefined);
  }

  protected open(id: string): void {
    void this.backend.showMainWindow(`/watches/${id}`).then(() => this.close(), () => undefined);
  }

  protected openMain(): void {
    void this.backend.showMainWindow(null).then(() => this.close(), () => undefined);
  }
}
