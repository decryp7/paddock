import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink, RouterLinkActive } from '@angular/router';
import type { ThemeChoice } from '../core/api.types';
import { LiveStore } from '../core/live.store';
import { ThemeService } from '../core/theme';
import { downloadActive, loadTone, servingLabel } from '../shared/labels';
import { bytes, percent } from '../shared/format';

interface NavItem {
  path: string;
  label: string;
  icon: string;
  exact?: boolean;
}

const GROUPS: { label: string; items: NavItem[] }[] = [
  { label: '', items: [{ path: '/', label: 'Dashboard', icon: 'dashboard', exact: true }] },
  {
    label: 'Serve',
    items: [
      { path: '/models', label: 'Models', icon: 'models' },
      { path: '/engines', label: 'llama.cpp', icon: 'engines' },
      { path: '/apps', label: 'Apps & keys', icon: 'apps' },
      { path: '/requests', label: 'Requests', icon: 'requests' },
    ],
  },
  {
    label: 'Try',
    items: [
      { path: '/playground', label: 'Playground', icon: 'playground' },
      { path: '/benchmarks', label: 'Benchmarks', icon: 'benchmarks' },
    ],
  },
];

/** The window's navigation: product, serving state, screens, memory at a glance and the theme switch. */
@Component({
  selector: 'pw-sidebar',
  imports: [MatButtonToggleGroup, MatButtonToggle, RouterLink, RouterLinkActive, MatIconModule, MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="brand" data-tauri-drag-region>
      <img class="logo" src="brand/pitwall-macos.svg" alt="" width="24" height="24" />
      <span class="product" data-tauri-drag-region>Pitwall</span>
      @if (live.server(); as s) {
        <span class="state" [attr.data-state]="s.serving" [matTooltip]="servingLabel(s.serving) + ' on port ' + s.port"><span class="dot" aria-hidden="true"></span>{{ servingLabel(s.serving) }}</span>
      } @else if (live.error()) {
        <span class="state" data-state="down"><span class="dot" aria-hidden="true"></span>Unreachable</span>
      } @else {
        <span class="state"><span class="dot" aria-hidden="true"></span>Connecting</span>
      }
    </div>

    <nav aria-label="Main">
      @for (g of groups; track g.label) {
        @if (g.label) {
          <div class="group">{{ g.label }}</div>
        }
        @for (item of g.items; track item.path) {
          <a class="item" [routerLink]="item.path" routerLinkActive="active" [routerLinkActiveOptions]="{ exact: !!item.exact }" ariaCurrentWhenActive="page">
            <mat-icon [svgIcon]="'pw:' + item.icon" aria-hidden="true" />
            <span class="label">{{ item.label }}</span>
            @if (item.path === '/apps' && pairings() > 0) {
              <span class="badge attention" [attr.aria-label]="pairings() + ' pairing requests'">{{ pairings() }}</span>
            }
            @if (item.path === '/models' && downloading() > 0) {
              <span class="badge" [attr.aria-label]="downloading() + ' downloads running'">
                <mat-icon svgIcon="pw:download" aria-hidden="true" />{{ downloading() }}
              </span>
            }
          </a>
        }
      }
    </nav>

    <div class="foot">
      @if (live.memory(); as m) {
        <a class="mem" routerLink="/" [attr.aria-label]="'Memory: ' + ramPct() + ' percent used'">
          <div class="mem-row"><span>RAM</span><span class="num">{{ ramUsed() }} / {{ ramTotal() }}</span></div>
          <div class="meter" [attr.data-tone]="tone(ramPct())"><span [style.width.%]="ramPct()"></span></div>
          @if (vramPct() !== null) {
            <div class="mem-row"><span>VRAM</span><span class="num">{{ vramText() }}</span></div>
            <div class="meter" [attr.data-tone]="tone(vramPct() ?? 0)"><span [style.width.%]="vramPct()"></span></div>
          }
        </a>
      }
      <div class="foot-row">
        <a class="item settings" routerLink="/settings" routerLinkActive="active" ariaCurrentWhenActive="page">
          <mat-icon svgIcon="pw:settings" aria-hidden="true" />
          <span class="label">Settings</span>
        </a>
        <mat-button-toggle-group class="theme" hideSingleSelectionIndicator aria-label="Theme" [value]="theme.choice()" (change)="theme.set($event.value)">
          @for (t of themes; track t.value) {
            <mat-button-toggle [value]="t.value" [aria-label]="t.label" [matTooltip]="t.label">
              <mat-icon [svgIcon]="'pw:' + t.icon" aria-hidden="true" />
            </mat-button-toggle>
          }
        </mat-button-toggle-group>
      </div>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      height: 100%;
      padding: calc(var(--pw-titlebar-h) + var(--pd-space-3)) var(--pd-space-2) var(--pd-space-3);
      background: var(--pd-bg);
      overflow: hidden;
      user-select: none;
    }
    .brand {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      padding: var(--pd-space-1) var(--pd-space-2) var(--pd-space-4);
    }
    .logo {
      width: 24px;
      height: 24px;
      flex: none;
    }
    .product {
      flex: 1;
      font-weight: var(--pd-weight-semibold);
      font-size: var(--pd-text-subtitle);
      letter-spacing: normal;
    }
    /* One pill for the serving state; the port is in its tooltip. */
    .state {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      height: 22px;
      padding: 0 8px;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      color: var(--pd-text-muted);
      background: var(--pd-pressed);
      border-radius: var(--pd-radius-pill);
      white-space: nowrap;
      .dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--pd-text-subtle);
      }
      &[data-state='serving'] .dot {
        background: var(--pd-success);
      }
      &[data-state='paused'] .dot,
      &[data-state='stopped'] .dot {
        background: var(--pd-warning);
      }
      &[data-state='down'] .dot {
        background: var(--pd-danger);
      }
    }
    nav {
      display: flex;
      flex-direction: column;
      gap: 1px;
      flex: 1;
      overflow: auto;
    }
    .group {
      padding: var(--pd-space-4) var(--pd-space-2) var(--pd-space-1);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      color: var(--pd-text-muted);
    }
    .item {
      display: flex;
      align-items: center;
      gap: 10px;
      height: 30px;
      padding: 0 var(--pd-space-2);
      color: var(--pd-text-muted);
      text-decoration: none;
      border-radius: var(--pd-radius-control);
      font-weight: var(--pd-weight-medium);
      transition:
        background var(--pd-duration-micro) var(--pd-ease-move),
        color var(--pd-duration-micro) var(--pd-ease-move);
      mat-icon {
        width: 16px;
        height: 16px;
        flex: none;
      }
      .label {
        flex: 1;
        white-space: nowrap;
      }
      &:hover {
        background: var(--pd-hover);
        color: var(--pd-text);
      }
      &.active {
        background: var(--pd-pressed);
        color: var(--pd-text);
        mat-icon {
          color: var(--pd-accent);
        }
      }
    }
    .badge {
      display: inline-flex;
      align-items: center;
      gap: 3px;
      min-width: 18px;
      height: 18px;
      padding: 0 5px;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-semibold);
      color: var(--pd-text-muted);
      background: var(--pd-pressed);
      border-radius: var(--pd-radius-pill);
      mat-icon {
        width: 11px;
        height: 11px;
      }
      /* Counts are neutral: the accent is for action and selection, not for numbers. */
      &.attention {
        justify-content: center;
        color: var(--pd-text);
        background: var(--pd-pressed);
      }
    }
    .foot {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-2);
      padding-top: var(--pd-space-2);
    }
    .mem {
      display: grid;
      gap: 5px;
      margin: 0 var(--pd-space-1);
      padding: var(--pd-space-3);
      color: inherit;
      text-decoration: none;
      background: var(--pd-hover);
      border-radius: var(--pd-radius-card);
      .mem-row {
        display: flex;
        justify-content: space-between;
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      .meter {
        height: 4px;
        margin-bottom: 4px;
      }
    }
    .foot-row {
      display: flex;
      align-items: center;
      gap: var(--pd-space-1);
      .settings {
        flex: 1;
      }
    }
    /* The theme switch: three icon segments, as small as the sidebar's footer row. */
    .theme {
      --mat-button-toggle-height: 24px;
      --mat-button-toggle-label-text-line-height: 24px;
      flex: none;
      mat-icon {
        width: 13px;
        height: 13px;
      }
    }
    .theme ::ng-deep .mat-button-toggle-label-content {
      padding: 0 6px;
    }
  `,
})
export class Sidebar {
  protected readonly live = inject(LiveStore);
  protected readonly theme = inject(ThemeService);
  protected readonly groups = GROUPS;
  protected readonly themes: { value: ThemeChoice; label: string; icon: string }[] = [
    { value: 'system', label: 'System theme', icon: 'monitor' },
    { value: 'light', label: 'Light theme', icon: 'sun' },
    { value: 'dark', label: 'Dark theme', icon: 'moon' },
  ];

  protected readonly pairings = computed(() => this.live.pairings().length);
  protected readonly downloading = computed(() => this.live.downloads().filter((d) => downloadActive(d.state)).length);
  protected readonly tone = loadTone;
  protected readonly servingLabel = servingLabel;
  protected readonly ramPct = computed(() => {
    const m = this.live.memory();
    return m ? percent(m.ramTotal - m.ramAvailable, m.ramTotal) : 0;
  });
  protected readonly ramUsed = computed(() => {
    const m = this.live.memory();
    return m ? bytes(m.ramTotal - m.ramAvailable) : '';
  });
  protected readonly ramTotal = computed(() => bytes(this.live.memory()?.ramTotal ?? 0, 0));
  protected readonly vramPct = computed(() => {
    const g = this.live.memory()?.gpus[0];
    return g && g.vramTotal && g.vramUsed !== null ? percent(g.vramUsed, g.vramTotal) : null;
  });
  protected readonly vramText = computed(() => {
    const g = this.live.memory()?.gpus[0];
    return g ? `${bytes(g.vramUsed)} / ${bytes(g.vramTotal, 0)}` : '';
  });
}
