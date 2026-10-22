import { ChangeDetectionStrategy, Component, ElementRef, computed, inject, viewChildren } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { BrandLogo } from '../shared/ui/brand-logo';
import { StatusStore } from '../state/status.store';
import { RegisterLauncher } from './register-launcher';

interface NavItem {
  path: string;
  label: string;
  icon: string;
  exact?: boolean;
}

const NAV: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: 'dashboard', exact: true },
  { path: '/runners', label: 'Runners', icon: 'runner' },
  { path: '/activity', label: 'Activity', icon: 'activity' },
  { path: '/insights', label: 'Insights', icon: 'insights' },
  { path: '/storage', label: 'Storage', icon: 'storage' },
  { path: '/logs', label: 'Logs', icon: 'logs' },
  { path: '/updates', label: 'Updates', icon: 'updates' },
  { path: '/settings', label: 'Settings', icon: 'settings' },
];

/** The navigation: links (Tab), with arrow keys moving between them (roving focus). */
@Component({
  selector: 'pc-sidebar',
  imports: [RouterLink, RouterLinkActive, MatIcon, BrandLogo],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="drag" data-tauri-drag-region></div>
    <div class="brand" data-tauri-drag-region>
      <pc-brand-logo [size]="26" />
      <span class="name">Pitcrew</span>
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
          <mat-icon [svgIcon]="'pc:' + item.icon" aria-hidden="true" />
          <span class="label">{{ item.label }}</span>
          @if (item.path === '/runners' && attention() > 0) {
            <span class="count danger" [attr.aria-label]="attention() + ' need attention'">{{ attention() }}</span>
          } @else if (item.path === '/runners' && runners() > 0) {
            <span class="count" aria-hidden="true">{{ runners() }}</span>
          }
          @if (item.path === '/updates' && updates() > 0) {
            <span class="count accent" [attr.aria-label]="updates() + ' updates available'">{{ updates() }}</span>
          }
        </a>
      }
    </nav>
    <div class="spacer"></div>
    <button type="button" class="btn register" (click)="register.open()">
      <mat-icon svgIcon="pc:plus" aria-hidden="true" />
      Register runner
    </button>
    <div class="service" [class]="serviceTone()">
      <span class="dot" aria-hidden="true"></span>
      <span class="truncate">{{ serviceText() }}</span>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
      height: 100%;
      padding: var(--space-3);
      padding-top: calc(var(--space-3) + var(--titlebar-inset));
      background: var(--sidebar-bg);
      border-right: 1px solid var(--border);
    }
    .drag {
      position: absolute;
      inset: 0 0 auto 0;
      height: var(--titlebar-inset);
    }
    .brand {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      height: 36px;
      padding: 0 var(--space-2);
      margin-bottom: var(--space-3);
    }
    .name {
      font-family: var(--font-display);
      font-weight: var(--weight-semibold);
      font-size: var(--text-lg);
      letter-spacing: -0.01em;
    }
    nav {
      display: grid;
      gap: 2px;
    }
    .item {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      height: 32px;
      padding: 0 var(--space-2);
      border-radius: var(--radius-sm);
      color: var(--text-2);
      font-weight: var(--weight-medium);
      text-decoration: none;
      transition:
        background var(--duration-fast) ease,
        color var(--duration-fast) ease;
    }
    .item:hover {
      background: var(--hover);
      color: var(--text);
      text-decoration: none;
    }
    .item.active {
      background: var(--active);
      color: var(--text);
    }
    .item.active mat-icon {
      color: var(--accent);
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
      height: 18px;
      padding: 0 6px;
      border-radius: var(--radius-full);
      background: var(--surface-3);
      color: var(--text-2);
      font-size: var(--text-xs);
      font-weight: var(--weight-semibold);
      line-height: 18px;
      text-align: center;
    }
    .count.danger {
      background: var(--danger-soft);
      color: var(--danger);
    }
    .count.accent {
      background: var(--accent-soft);
      color: var(--accent);
    }
    .spacer {
      flex: 1;
    }
    .register {
      width: 100%;
      margin-bottom: var(--space-2);
    }
    .service {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      padding: var(--space-2);
      font-size: var(--text-sm);
      color: var(--text-3);
    }
    .service .dot {
      color: var(--text-3);
    }
    .service.success .dot {
      color: var(--success);
    }
    .service.danger .dot {
      color: var(--danger);
    }
  `,
})
export class Sidebar {
  protected readonly nav = NAV;
  protected readonly register = inject(RegisterLauncher);
  private readonly store = inject(StatusStore);
  private readonly links = viewChildren<ElementRef<HTMLAnchorElement>>('link');

  protected readonly runners = computed(() => this.store.runners().length);
  protected readonly attention = computed(() => this.store.attention().length);
  protected readonly updates = computed(() => this.store.runners().filter((r) => r.updateTarget !== null && r.updatePhase.phase === 'idle').length);
  protected readonly serviceTone = computed(() => {
    const c = this.store.connection();
    return c.state === 'connected' ? 'success' : c.state === 'connecting' ? 'neutral' : 'danger';
  });
  protected readonly serviceText = computed(() => {
    const c = this.store.connection();
    switch (c.state) {
      case 'connected':
        return `Service ${c.serviceVersion} · connected`;
      case 'connecting':
        return 'Connecting to the service…';
      case 'mismatch':
        return 'Service version mismatch';
      case 'unavailable':
        return 'Service unavailable';
    }
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
