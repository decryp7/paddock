import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, inject, signal } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatTooltip } from '@angular/material/tooltip';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
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
  imports: [MatButton, MatIconButton, MatIcon, MatMenu, MatMenuItem, MatMenuTrigger, MatTooltip, SuTrayPopover, PdWidgetArea, WidgetSet, StatusPill],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Pitwall" [platform]="platform" (dismissed)="close()">
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
      <div slot="footer" class="foot">
        <span class="spacer"></span>
        <button matButton="filled" type="button" (click)="open('/')"><mat-icon svgIcon="pw:arrow-right" aria-hidden="true" />Open Pitwall</button>
        <button matIconButton type="button" aria-label="More actions" matTooltip="More actions" matTooltipPosition="left" [matMenuTriggerFor]="more">
          <mat-icon svgIcon="pw:more" aria-hidden="true" />
        </button>
        <mat-menu #more="matMenu" yPosition="above" xPosition="before">
          <button mat-menu-item type="button" (click)="customize()"><mat-icon svgIcon="pw:sliders" aria-hidden="true" />Customize widgets…</button>
          @if (serving(); as s) {
            <button mat-menu-item type="button" (click)="setServing(s === 'pause')">
              <mat-icon [svgIcon]="s === 'pause' ? 'pw:pause' : 'pw:play'" aria-hidden="true" />{{ s === 'pause' ? 'Pause serving' : 'Resume serving' }}
            </button>
          }
          <button mat-menu-item type="button" [disabled]="!widgets.header()?.canReleaseMemory" (click)="releaseMemory()"><mat-icon svgIcon="pw:release" aria-hidden="true" />Release memory</button>
          <button mat-menu-item type="button" (click)="open(settingsRoute)"><mat-icon svgIcon="pw:settings" aria-hidden="true" />Settings…</button>
          <hr class="sep" />
          <button mat-menu-item type="button" (click)="quit()"><mat-icon svgIcon="pw:power" aria-hidden="true" />Quit Pitwall</button>
        </mat-menu>
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
    .foot {
      display: flex;
      align-items: center;
      width: 100%;
      gap: var(--pd-space-2);
    }
    .spacer {
      flex: 1;
    }
    .sep {
      border: 0;
      border-top: 1px solid var(--pd-hairline);
      margin: 4px 0;
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
  protected readonly settingsRoute = SETTINGS_GENERAL_ROUTE;
  protected readonly platform = platformOf(navigator.userAgent);
  /** What the last button press that failed said; cleared by the next press. */
  protected readonly actionError = signal<string | null>(null);
  /** What the footer's Pause or Resume item does now; none while the server cannot be reached (the engine decides). */
  protected readonly serving = computed(() => {
    const t = this.widgets.header()?.serving;
    return t === 'pause' || t === 'resume' ? t : null;
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

  protected setServing(pause: boolean): void {
    this.attempt(this.backend.setServing(pause));
  }

  protected releaseMemory(): void {
    this.attempt(this.backend.releaseMemory());
  }

  protected quit(): void {
    this.attempt(this.windows.quit());
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
