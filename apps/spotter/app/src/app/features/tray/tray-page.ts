import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { SuEmptyState, SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { BACKEND } from '../../core/backend';
import { platformFromUserAgent } from '../../core/tauri-backend';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { WatchesStore } from '../../state/watches.store';

/** The tray popup: unseen releases at a glance. Frameless and transparent; the shell draws the OS material behind. */
@Component({
  selector: 'sp-tray-page',
  imports: [MatIcon, SuEmptyState, SuTrayPopover, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Spotter" [platform]="platform" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/spotter-small.svg" alt="" width="20" height="20" />
        <strong>Spotter</strong>
        @if (unseen().length) {
          <span class="pill" role="status">{{ unseen().length }} unseen</span>
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
                <span class="main">
                  <span class="name truncate">{{ w.name }}</span>
                  <span class="meta truncate"><span class="ver">{{ w.latest?.version }}</span> · {{ w.latest?.firstSeenMs | ago }}</span>
                </span>
                <mat-icon svgIcon="sp:chevron-right" aria-hidden="true" />
              </button>
              <button type="button" class="btn ghost icon sm" (click)="store.acknowledge(w.id, null)" [attr.aria-label]="'Mark ' + w.name + ' as seen'"><mat-icon svgIcon="sp:check" aria-hidden="true" /></button>
            </li>
          }
        </ul>
      } @else {
        <pd-empty-state heading="Nothing new" description="You have seen every release." />
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
      background: var(--pd-accent-tint);
      color: var(--pd-accent);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
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
    .item mat-icon {
      width: 14px;
      height: 14px;
      color: var(--pd-text-subtle);
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
  protected readonly platform: SuTrayPlatform = (() => {
    const p = platformFromUserAgent(navigator.userAgent);
    return p === 'windows' ? 'windows' : p === 'linux' ? 'linux' : 'macos';
  })();

  constructor() {
    this.store.start();
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
