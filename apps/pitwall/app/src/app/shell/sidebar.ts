import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
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
  imports: [RouterLink, RouterLinkActive, MatIconModule, MatTooltipModule],
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
        <div class="theme" role="radiogroup" aria-label="Theme">
          @for (t of themes; track t.value) {
            <button type="button" role="radio" [attr.aria-checked]="theme.choice() === t.value" [attr.aria-label]="t.label" [matTooltip]="t.label" (click)="theme.set(t.value)">
              <mat-icon [svgIcon]="'pw:' + t.icon" aria-hidden="true" />
            </button>
          }
        </div>
      </div>
    </div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      height: 100%;
      padding: calc(var(--titlebar-h) + var(--sp-3)) var(--sp-2) var(--sp-3);
      background: var(--bg-sidebar);
      overflow: hidden;
      user-select: none;
    }
    .brand {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      padding: var(--sp-1) var(--sp-2) var(--sp-4);
    }
    .logo {
      width: 24px;
      height: 24px;
      flex: none;
    }
    .product {
      flex: 1;
      font-weight: var(--fw-semibold);
      font-size: var(--fs-lg);
      letter-spacing: var(--tracking-tight);
    }
    /* One pill for the serving state; the port is in its tooltip. */
    .state {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      height: 22px;
      padding: 0 8px;
      font-size: var(--fs-xs);
      font-weight: var(--fw-medium);
      color: var(--text-2);
      background: var(--bg-active);
      border-radius: var(--r-full);
      white-space: nowrap;
      .dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--text-3);
      }
      &[data-state='serving'] .dot {
        background: var(--ok);
      }
      &[data-state='paused'] .dot,
      &[data-state='stopped'] .dot {
        background: var(--warn);
      }
      &[data-state='down'] .dot {
        background: var(--bad);
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
      padding: var(--sp-4) var(--sp-2) var(--sp-1);
      font-size: var(--fs-xs);
      font-weight: var(--fw-medium);
      color: var(--text-2);
    }
    .item {
      display: flex;
      align-items: center;
      gap: 10px;
      height: 30px;
      padding: 0 var(--sp-2);
      color: var(--text-2);
      text-decoration: none;
      border-radius: var(--r-md);
      font-weight: var(--fw-medium);
      transition:
        background var(--dur-fast) var(--ease),
        color var(--dur-fast) var(--ease);
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
        background: var(--bg-hover);
        color: var(--text);
      }
      &.active {
        background: var(--bg-active);
        color: var(--text);
        mat-icon {
          color: var(--accent);
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
      font-size: var(--fs-2xs);
      font-weight: var(--fw-semibold);
      color: var(--text-2);
      background: var(--bg-active);
      border-radius: var(--r-full);
      mat-icon {
        width: 11px;
        height: 11px;
      }
      /* Counts are neutral: the accent is for action and selection, not for numbers. */
      &.attention {
        justify-content: center;
        color: var(--text);
        background: var(--bg-active);
      }
    }
    .foot {
      display: flex;
      flex-direction: column;
      gap: var(--sp-2);
      padding-top: var(--sp-2);
    }
    .mem {
      display: grid;
      gap: 5px;
      margin: 0 var(--sp-1);
      padding: var(--sp-3);
      color: inherit;
      text-decoration: none;
      background: var(--bg-hover);
      border-radius: var(--r-lg);
      .mem-row {
        display: flex;
        justify-content: space-between;
        font-size: var(--fs-xs);
        color: var(--text-2);
      }
      .meter {
        height: 4px;
        margin-bottom: 4px;
      }
    }
    .foot-row {
      display: flex;
      align-items: center;
      gap: var(--sp-1);
      .settings {
        flex: 1;
      }
    }
    .theme {
      display: flex;
      padding: 2px;
      background: var(--bg-hover);
      border-radius: var(--r-md);
      button {
        display: grid;
        place-items: center;
        width: 24px;
        height: 22px;
        padding: 0;
        color: var(--text-3);
        background: none;
        border: none;
        border-radius: var(--r-sm);
        cursor: default;
        mat-icon {
          width: 13px;
          height: 13px;
        }
        &[aria-checked='true'] {
          color: var(--text);
          background: var(--bg-card);
          box-shadow: var(--shadow-1);
        }
      }
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
