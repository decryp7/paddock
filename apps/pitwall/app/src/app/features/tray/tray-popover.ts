import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { SuTrayPopover, type SuTrayMenuEntry, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { PdWidgetArea } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import type { WidgetActionId } from '../../core/generated/widgets';
import { SETTINGS_GENERAL_ROUTE, SETTINGS_TRAY_POPUP_ROUTE } from '../../core/navigation';
import { WINDOW_CONTROL } from '../../core/window-control';
import { observeHeight } from '../../shared/observe-height';
import { StatusPill } from './status-pill';
import { TrayWidgetsStore } from './tray-widgets.store';
import { WidgetSet } from './widget-set';

/** The ids of the footer's more-actions menu. */
type TrayMenuId = 'customize' | 'pause' | 'resume' | 'release' | 'settings' | 'quit';

/** The platform the popover is drawn for (the shell fits the window; the frame's corners and shadow follow the OS). */
function platformOf(userAgent: string): SuTrayPlatform {
  return /Windows/i.test(userAgent) ? 'windows' : /Linux|X11/i.test(userAgent) && !/Mac/i.test(userAgent) ? 'linux' : 'macos';
}

/**
 * The tray / menu-bar popup (family ADR 0035): the person's widgets on the shared widget area, in the shared popover. The
 * layout and every value are the app's (computed in Rust, `pitwall_core::widgets`); this page draws them and sends the action a
 * button names to the command that does it.
 */
@Component({
  selector: 'pw-tray-popover',
  imports: [MatButton, MatIcon, SuTrayPopover, PdWidgetArea, WidgetSet, StatusPill],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Pitwall" [platform]="platform" primaryLabel="Open Pitwall" [menu]="menu()" (primary)="open('/')" (menuAction)="choose($event)" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/pitwall-macos.svg" alt="" width="16" height="16" />
        <h1>Pitwall</h1>
        <pw-status-pill [status]="widgets.header()?.status ?? null" />
      </div>
      <pw-widget-set #set (run)="perform($event)" />
      @if (actionError(); as message) {
        <p class="error" role="alert"><mat-icon svgIcon="pw:alert" aria-hidden="true" />{{ message }}</p>
      }
      @if (widgets.view(); as view) {
        <pd-widget-area
          [placements]="view.layout.placements"
          [catalogue]="view.catalogue"
          [values]="widgets.values()"
          [templates]="set.widgets()"
          [meta]="set.meta"
          (action)="perform($event)"
          (customize)="customize()"
        />
      } @else if (widgets.error(); as err) {
        <div class="state" role="alert">
          <mat-icon svgIcon="pw:alert" aria-hidden="true" />
          <strong>Couldn't read the popup</strong>
          <span class="muted">{{ err.message }}</span>
          <button matButton="outlined" type="button" (click)="widgets.load()">Retry</button>
        </div>
      } @else {
        <div class="state" role="status" aria-busy="true">
          <span class="sk" aria-hidden="true"></span>
          <span class="sk" aria-hidden="true"></span>
          <span class="sr">Loading the popup</span>
        </div>
      }
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
    h1 {
      margin: 0;
      font-size: var(--pd-text-body);
      font-weight: var(--pd-weight-semibold);
    }
    .state {
      display: grid;
      justify-items: start;
      gap: var(--pd-space-2);
      padding: var(--pd-space-3);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-2);
    }
    .state mat-icon {
      width: 16px;
      height: 16px;
      color: var(--pd-danger);
    }
    .muted {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
    }
    .sk {
      display: block;
      width: 100%;
      height: 56px;
      border-radius: var(--pd-radius-card);
      background: var(--pd-skeleton);
    }
    .sr {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
    }
    .error {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      margin: 0 0 var(--pd-space-2);
      padding: var(--pd-space-2) var(--pd-space-3);
      border-radius: var(--pd-radius-card);
      background: var(--pd-danger-tint);
      font-size: var(--pd-text-label);
    }
    .error mat-icon {
      width: 14px;
      height: 14px;
      color: var(--pd-danger);
    }
    @media (prefers-reduced-motion: no-preference) {
      .sk {
        animation: sk-pulse 1.2s ease-in-out infinite;
      }
    }
    @keyframes sk-pulse {
      50% {
        opacity: 0.6;
      }
    }
  `,
})
export class TrayPopover {
  protected readonly widgets = inject(TrayWidgetsStore);
  private readonly backend = inject(BACKEND);
  private readonly windows = inject(WINDOW_CONTROL);
  protected readonly platform = platformOf(navigator.userAgent);
  /** What the last button press that failed said; cleared by the next press. */
  protected readonly actionError = signal<string | null>(null);
  /** What the footer's Pause or Resume item does now; none while the server cannot be reached (the engine decides). */
  protected readonly serving = computed(() => {
    const t = this.widgets.header()?.serving;
    return t === 'pause' || t === 'resume' ? t : null;
  });
  /** The footer's more-actions menu (the shared footer): what the engine allows now, Settings, then Quit after a separator. */
  protected readonly menu = computed<readonly SuTrayMenuEntry<TrayMenuId>[]>(() => {
    const serving = this.serving();
    return [
      { kind: 'item', id: 'customize', label: 'Customize widgets…', icon: 'pw:sliders' },
      ...(serving
        ? [{ kind: 'item', id: serving, label: serving === 'pause' ? 'Pause serving' : 'Resume serving', icon: serving === 'pause' ? 'pw:pause' : 'pw:play' } as const]
        : []),
      { kind: 'item', id: 'release', label: 'Release memory', icon: 'pw:release', disabled: !this.widgets.header()?.canReleaseMemory },
      { kind: 'item', id: 'settings', label: 'Settings…', icon: 'pw:settings' },
      { kind: 'separator' },
      { kind: 'item', id: 'quit', label: 'Quit Pitwall', icon: 'pw:power' },
    ];
  });

  constructor() {
    this.widgets.start();
    // The popover is as tall as its content: the shell fits the window to the height the page reports (and clamps it).
    const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    const destroy = inject(DestroyRef);
    afterNextRender(() => {
      const popup = host.querySelector('pd-tray-popover');
      if (popup) destroy.onDestroy(observeHeight(popup, (h) => void this.windows.fitTray(h).catch(() => undefined)));
    });
  }

  protected close(): void {
    void this.windows.closeTray().catch(() => undefined);
  }

  protected open(route: string): void {
    this.attempt(this.windows.openMain(route));
  }

  /** "Customize widgets…": the Settings window, at the popup's page (the main window opens it, or points the open one there). */
  protected customize(): void {
    this.open(SETTINGS_TRAY_POPUP_ROUTE);
  }

  /** What each footer menu item does: one entry per id, so a new id does not compile until it has its action. */
  private readonly menuActions: Record<TrayMenuId, () => void> = {
    customize: () => this.customize(),
    pause: () => this.attempt(this.backend.setServing(true)),
    resume: () => this.attempt(this.backend.setServing(false)),
    release: () => this.attempt(this.backend.releaseMemory()),
    settings: () => this.open(SETTINGS_GENERAL_ROUTE),
    quit: () => this.attempt(this.windows.quit()),
  };

  /** Runs a footer menu item. */
  protected choose(id: TrayMenuId): void {
    this.menuActions[id]();
  }

  /** Runs what a widget asked for through the command that does it; the app decides the rest. */
  protected perform(action: WidgetActionId): void {
    switch (action.type) {
      case 'openPage':
        return this.open(`/${action.page}`);
      case 'cancelDownload':
        return this.attempt(this.backend.cancelDownload(action.id));
    }
  }

  private attempt(work: Promise<unknown>): void {
    this.actionError.set(null);
    work.catch((e: unknown) => this.actionError.set(toCommandError(e).message));
  }
}
