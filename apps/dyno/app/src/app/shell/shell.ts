import { ChangeDetectionStrategy, Component, DestroyRef, OnInit, computed, inject, signal } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { BACKEND } from '../core/backend';
import { PlatformService } from '../core/platform';
import { Dialogs } from '../shared/dialog';
import { formatBytes, formatDuration, formatPercent } from '../shared/format';
import { Icon } from '../shared/icon';
import { SEVERITY_TONE } from '../shared/labels';
import { Notifier, Toasts } from '../shared/notifier';
import { LiveStore } from '../state/live.store';
import { SettingsStore } from '../state/settings.store';
import { CommandPalette } from './command-palette';
import { NAV } from './nav';
import { ProfileSwitcher } from './profile-switcher';

const COLLAPSED_KEY = 'dyno.sidebarCollapsed';

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === '1';
  } catch {
    return false;
  }
}

/** The main window: sidebar navigation, the top bar (search, profile, agent status, Dyno's overhead) and the page. */
@Component({
  selector: 'dy-shell',
  imports: [RouterOutlet, RouterLink, RouterLinkActive, Icon, ProfileSwitcher, Toasts, MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.collapsed]': 'collapsed()',
    '[class.mac]': 'platform.overlayTitleBar',
    '(document:keydown)': 'shortcut($event)',
  },
  template: `
    <a class="skip" href="#main" (click)="skip($event)">Skip to content</a>
    <aside class="sidebar" aria-label="Navigation">
      @if (platform.overlayTitleBar) {
        <div class="traffic" data-tauri-drag-region></div>
      }
      <div class="brand" data-tauri-drag-region>
        <img class="mark" src="dyno-logo.svg" alt="" width="26" height="26" />
        <span class="word">Dyno</span>
      </div>
      <nav>
        <ul>
          @for (item of nav; track item.path) {
            <li>
              <a
                [routerLink]="item.path"
                routerLinkActive="active"
                [routerLinkActiveOptions]="{ exact: item.path === '/' }"
                ariaCurrentWhenActive="page"
                [matTooltip]="collapsed() ? item.label : ''"
                matTooltipPosition="right"
                [attr.aria-label]="collapsed() ? item.label : null"
              >
                <dy-icon [name]="item.icon" />
                <span class="label">{{ item.label }}</span>
                @if (item.planned) {
                  <span class="soon label">Soon</span>
                }
              </a>
            </li>
          }
        </ul>
      </nav>
      <div class="side-foot">
        <button type="button" class="btn ghost collapse" (click)="toggle()" [attr.aria-label]="collapsed() ? 'Expand sidebar' : 'Collapse sidebar'" [attr.aria-expanded]="!collapsed()">
          <dy-icon name="sidebar" />
          <span class="label">Collapse</span>
        </button>
      </div>
    </aside>

    <div class="main">
      <header class="topbar" [attr.data-tauri-drag-region]="platform.overlayTitleBar ? '' : null">
        <button type="button" class="search" (click)="openPalette()" aria-haspopup="dialog" aria-label="Search and commands">
          <dy-icon name="search" />
          <span>Search apps, screens and actions…</span>
          <span class="keys"><kbd>{{ platform.mod }}</kbd><kbd>K</kbd></span>
        </button>
        <span class="spacer" [attr.data-tauri-drag-region]="platform.overlayTitleBar ? '' : null"></span>
        <div class="overhead num" [matTooltip]="overheadTip()" role="status" aria-label="Dyno's own resource use">
          <dy-icon name="gauge" />
          <span>{{ overheadText() }}</span>
        </div>
        <dy-profile-switcher />
        <div class="agent" [attr.data-state]="live.agent().state" role="status" [matTooltip]="live.agent().message ?? ''">
          <span class="pulse"></span>
          <span>{{ agentText() }}</span>
        </div>
      </header>
      <main id="main" class="content" tabindex="-1">
        <router-outlet />
      </main>
    </div>
    <dy-toasts />
  `,
  styles: `
    :host { display: grid; grid-template-columns: var(--sidebar-w) minmax(0, 1fr); height: 100vh; background: var(--bg); transition: grid-template-columns var(--dur-base) var(--ease-out); }
    :host(.collapsed) { grid-template-columns: var(--sidebar-w-collapsed) minmax(0, 1fr); }
    .skip { position: absolute; left: -9999px; top: 8px; z-index: 2000; }
    .skip:focus { left: 8px; background: var(--surface-2); padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); }

    .sidebar { display: flex; flex-direction: column; min-width: 0; background: var(--surface-1); border-right: 1px solid var(--border); padding: var(--sp-3) var(--sp-2); gap: var(--sp-2); overflow: hidden; }
    .traffic { height: var(--titlebar-inset); flex: none; }
    .brand { display: flex; align-items: center; gap: var(--sp-2); height: 36px; padding: 0 var(--sp-2); }
    .mark { width: 26px; height: 26px; flex: none; }
    .word { font: var(--fw-bold) var(--fs-lg) / 1 var(--font-display); letter-spacing: -0.02em; }
    nav ul { list-style: none; margin: var(--sp-2) 0 0; padding: 0; display: grid; gap: 2px; }
    nav a { display: flex; align-items: center; gap: var(--sp-3); height: 32px; padding: 0 var(--sp-2) 0 10px; border-radius: var(--r-md); color: var(--text-2); font-weight: var(--fw-medium); text-decoration: none; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); white-space: nowrap; }
    nav a:hover { background: var(--surface-hover); color: var(--text-1); text-decoration: none; }
    nav a.active { background: var(--surface-2); color: var(--text-1); box-shadow: var(--shadow-1), 0 0 0 1px var(--border); }
    nav a.active dy-icon { color: var(--accent-text); }
    nav a dy-icon { --icon-size: 18px; }
    .side-foot { margin-top: auto; display: grid; gap: var(--sp-1); }
    .overhead { display: flex; align-items: center; gap: 6px; height: var(--control-h-sm); padding: 0 var(--sp-2); border-radius: var(--r-pill); background: var(--surface-inset); border: 1px solid var(--border); color: var(--text-2); font-size: var(--fs-xs); white-space: nowrap; }
    .overhead dy-icon { --icon-size: 13px; color: var(--text-3); }
    .soon { margin-left: auto; font-size: var(--fs-2xs); font-weight: var(--fw-medium); color: var(--text-3); letter-spacing: 0.02em; }
    .collapse { justify-content: flex-start; gap: var(--sp-3); padding: 0 10px; color: var(--text-3); }
    :host(.collapsed) .label, :host(.collapsed) .word { display: none; }
    :host(.collapsed) .brand { justify-content: center; padding: 0; }
    :host(.collapsed) nav a, :host(.collapsed) .collapse { justify-content: center; padding: 0; }

    .main { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--content-bg); }
    .topbar { display: flex; align-items: center; gap: var(--sp-3); height: var(--topbar-h); padding: 0 var(--sp-4) 0 var(--sp-6); border-bottom: 1px solid var(--border); flex: none; }
    .search { display: flex; align-items: center; gap: var(--sp-2); width: min(420px, 40vw); height: var(--control-h); padding: 0 var(--sp-2) 0 var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-3); font: var(--fs-base) var(--font-sans); cursor: pointer; text-align: left; transition: border-color var(--dur-fast) var(--ease-out); }
    .search:hover { border-color: var(--border-strong); }
    .search dy-icon { --icon-size: 15px; }
    .search > span:first-of-type { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .keys { display: flex; gap: 2px; }
    .spacer { flex: 1; align-self: stretch; }
    .agent { display: flex; align-items: center; gap: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); white-space: nowrap; }
    .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent); }
    .agent[data-state='starting'] .pulse, .agent[data-state='restarting'] .pulse { background: var(--warn); box-shadow: 0 0 0 3px color-mix(in srgb, var(--warn) 22%, transparent); animation: dy-pulse 1.2s ease-in-out infinite; }
    .agent[data-state='failed'] .pulse { background: var(--bad); box-shadow: 0 0 0 3px color-mix(in srgb, var(--bad) 22%, transparent); }
    .content { flex: 1; overflow: auto; min-height: 0; outline: none; scrollbar-gutter: stable; }
    @media (max-width: 1100px) { .agent span:last-child { display: none; } }
    @media (max-width: 1040px) { .overhead span { display: none; } .overhead { padding: 0 6px; } }
  `,
})
export class Shell implements OnInit {
  protected readonly platform = inject(PlatformService);
  protected readonly live = inject(LiveStore);
  private readonly settings = inject(SettingsStore);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);
  private readonly backend = inject(BACKEND);
  protected readonly nav = NAV;
  protected readonly collapsed = signal(readCollapsed());
  private paletteOpen = false;

  /** Dyno's own cost, measured by the agent and always on screen: "Dyno: 0.4% CPU · 38 MB". */
  protected readonly overheadText = computed(() => {
    const o = this.live.overhead();
    return o ? `Dyno: ${formatPercent(o.agentCpu)} CPU · ${formatBytes(o.agentMemory)}` : 'Dyno: measuring…';
  });
  protected readonly overheadTip = computed(() => {
    const o = this.live.overhead();
    return o ? `Dyno's own use, measured by its agent (CPU is a share of one core). Sampling every ${formatDuration(o.samplingIntervalMs / 1000)}, processes every ${formatDuration(o.processIntervalMs / 1000)}.` : '';
  });

  protected readonly agentText = computed(() => {
    switch (this.live.agent().state) {
      case 'running':
        return 'Monitoring';
      case 'starting':
        return 'Starting…';
      case 'restarting':
        return 'Reconnecting…';
      default:
        return 'Agent stopped';
    }
  });

  constructor() {
    const stop = this.backend.onAlert((a) => this.notifier.show(a.title, SEVERITY_TONE[a.severity], null, 8000));
    inject(DestroyRef).onDestroy(stop);
  }

  ngOnInit(): void {
    void this.settings.load();
  }

  protected toggle(): void {
    const next = !this.collapsed();
    this.collapsed.set(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, next ? '1' : '0');
    } catch {
      // Private mode or blocked storage: the sidebar just forgets.
    }
  }

  protected openPalette(): void {
    if (this.paletteOpen) return;
    this.paletteOpen = true;
    const ref = this.dialogs.open(CommandPalette, undefined, { ariaLabelledBy: null, ariaLabel: 'Command palette', panelClass: ['dy-dialog-panel', 'dy-palette-panel'] });
    ref.closed.subscribe(() => (this.paletteOpen = false));
  }

  protected shortcut(e: KeyboardEvent): void {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      this.openPalette();
    }
  }

  protected skip(e: Event): void {
    e.preventDefault();
    document.getElementById('main')?.focus();
  }
}
