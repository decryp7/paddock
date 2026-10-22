import { ChangeDetectionStrategy, Component, ElementRef, computed, inject, viewChildren } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { ClockService } from '../core/clock.service';
import { spanText } from '../shared/format';
import { SettingsStore } from '../state/settings.store';
import { WatchesStore } from '../state/watches.store';

interface NavItem {
  path: string;
  label: string;
  icon: string;
  exact?: boolean;
}

const NAV: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: 'dashboard', exact: true },
  { path: '/watches', label: 'Watches', icon: 'watches' },
  { path: '/settings', label: 'Settings', icon: 'settings' },
];

/** The navigation: links (Tab), with arrow keys moving between them (roving focus). */
@Component({
  selector: 'sp-sidebar',
  imports: [RouterLink, RouterLinkActive, MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="drag" data-tauri-drag-region></div>
    <div class="brand" data-tauri-drag-region>
      <img [src]="logo" alt="" width="26" height="26" draggable="false" />
      <span class="name">Spotter</span>
    </div>
    <nav aria-label="Main">
      @for (item of nav; track item.path) {
        <a
          #link
          (keydown)="onKey($event)"
          class="item"
          [routerLink]="item.path"
          routerLinkActive="active"
          [routerLinkActiveOptions]="{ exact: !!item.exact }"
          ariaCurrentWhenActive="page"
        >
          <mat-icon [svgIcon]="'sp:' + item.icon" aria-hidden="true" />
          <span class="label">{{ item.label }}</span>
          @if (item.path === '/watches' && unseen() > 0) {
            <span class="count" [attr.aria-label]="unseen() + ' unseen'">{{ unseen() }}</span>
          }
        </a>
      }
    </nav>
    <div class="spacer"></div>
    @if (pausedUntil(); as until) {
      <div class="paused" role="status">
        <mat-icon svgIcon="sp:bell-off" aria-hidden="true" />
        <span>Notifications paused for another {{ until }}</span>
      </div>
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-1);
      height: 100%;
      padding: var(--pd-space-3);
      padding-top: calc(var(--pd-space-3) + var(--sp-titlebar-inset));
      background: var(--pd-surface-2);
      border-right: 1px solid var(--pd-hairline);
    }
    .drag {
      position: absolute;
      inset: 0 0 auto 0;
      height: var(--sp-titlebar-inset);
    }
    .brand {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      height: 36px;
      padding: 0 var(--pd-space-2);
      margin-bottom: var(--pd-space-3);
    }
    .name {
      font-weight: var(--pd-weight-semibold);
      font-size: var(--pd-text-subtitle);
    }
    nav {
      display: grid;
      gap: 2px;
    }
    .item {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      height: 32px;
      padding: 0 var(--pd-space-2);
      border-radius: var(--pd-radius-control);
      color: var(--pd-text-muted);
      font-weight: var(--pd-weight-medium);
      text-decoration: none;
    }
    .item:hover {
      background: var(--pd-hover);
      color: var(--pd-text);
    }
    .item.active {
      background: var(--pd-accent-tint);
      color: var(--pd-text);
    }
    .item.active mat-icon {
      color: var(--pd-accent);
    }
    .item mat-icon {
      width: 18px;
      height: 18px;
    }
    .label {
      flex: 1;
    }
    .count {
      min-width: 20px;
      padding: 0 6px;
      border-radius: var(--pd-radius-pill);
      background: var(--pd-accent-fill);
      color: var(--pd-on-accent);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
      text-align: center;
    }
    .spacer {
      flex: 1;
    }
    .paused {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      padding: var(--pd-space-2);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .paused mat-icon {
      flex: none;
      width: 16px;
      height: 16px;
    }
  `,
})
export class Sidebar {
  protected readonly nav = NAV;
  protected readonly logo = `brand/spotter-${document.documentElement.dataset['platform'] === 'macos' ? 'macos' : 'windows'}.svg`;
  private readonly watches = inject(WatchesStore);
  private readonly settings = inject(SettingsStore);
  private readonly clock = inject(ClockService);
  private readonly links = viewChildren<ElementRef<HTMLAnchorElement>>('link');

  protected readonly unseen = computed(() => this.watches.unseen().length);
  protected readonly pausedUntil = computed(() => {
    const until = this.settings.settings().value?.notificationsPausedUntilMs ?? null;
    return until !== null && until > this.clock.now() ? spanText(until - this.clock.now()) : null;
  });

  protected onKey(e: KeyboardEvent): void {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End') return;
    const links = this.links().map((l) => l.nativeElement);
    const i = links.indexOf(document.activeElement as HTMLAnchorElement);
    const n = links.length;
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : e.key === 'ArrowDown' ? (i + 1) % n : (i - 1 + n) % n;
    links[next]?.focus();
    e.preventDefault();
  }
}
