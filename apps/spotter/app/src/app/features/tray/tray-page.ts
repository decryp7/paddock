import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, inject, signal } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { PdWidgetArea } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import type { WidgetActionId } from '../../core/generated/widgets';
import { platformFromUserAgent } from '../../core/tauri-backend';
import { observeHeight } from '../../shared/observe-height';
import { TrayWidgetsStore } from '../../state/tray-widgets.store';
import { WatchesStore } from '../../state/watches.store';
import { WidgetSet } from './widget-set';

/**
 * The tray popup (family ADR 0035, Spotter's catalogue in `spotter-core`'s `widgets`): the person's widgets on the shared
 * widget area, in the shared popover. Frameless and transparent; the shell draws the OS material behind. The layout and every
 * value are the engine's; this page draws them and sends the action a button names to the command that does it.
 */
@Component({
  selector: 'sp-tray-page',
  imports: [MatButton, MatIconButton, MatIcon, MatMenu, MatMenuItem, MatMenuTrigger, SuTrayPopover, PdWidgetArea, WidgetSet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Spotter" [platform]="platform" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/spotter-16.svg" alt="" width="16" height="16" />
        <strong>Spotter</strong>
        @if (unseen() > 0) {
          <span class="pill" role="status"><mat-icon svgIcon="sp:sparkle" aria-hidden="true" />{{ unseen() }} new</span>
        }
      </div>
      <sp-widget-set #set (run)="perform($event)" />
      @if (actionError(); as message) {
        <p class="error" role="alert"><mat-icon svgIcon="sp:alert" aria-hidden="true" />{{ message }}</p>
      }
      @if (widgets.view(); as view) {
        <pd-widget-area
          [placements]="view.layout.placements"
          [catalogue]="view.catalogue"
          [values]="widgets.values()"
          [templates]="set.widgets()"
          (action)="perform($event)"
          (customize)="customize()"
        />
      } @else if (widgets.error(); as err) {
        <div class="state" role="alert">
          <mat-icon svgIcon="sp:alert" aria-hidden="true" />
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
        <button matButton="filled" type="button" (click)="openMain()">Open Spotter</button>
        <span class="spacer"></span>
        <button matIconButton type="button" aria-label="More actions" [matMenuTriggerFor]="more" yPosition="above">
          <mat-icon svgIcon="sp:more" aria-hidden="true" />
        </button>
        <mat-menu #more="matMenu" yPosition="above">
          <button mat-menu-item type="button" (click)="customize()"><mat-icon svgIcon="sp:settings" aria-hidden="true" />Customize widgets…</button>
          <button mat-menu-item type="button" [disabled]="unseen() === 0" (click)="watches.acknowledgeAll()"><mat-icon svgIcon="sp:check-all" aria-hidden="true" />Mark all as seen</button>
          <button mat-menu-item type="button" (click)="perform({ type: 'openSettings' })"><mat-icon svgIcon="sp:settings" aria-hidden="true" />Settings…</button>
          <button mat-menu-item type="button" (click)="quit()"><mat-icon svgIcon="sp:x" aria-hidden="true" />Quit Spotter</button>
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
    .pill {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-1);
      margin-left: auto;
      padding: 1px var(--pd-space-2);
      border-radius: var(--pd-radius-pill);
      background: var(--pd-surface-3);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
    }
    .pill mat-icon {
      width: 12px;
      height: 12px;
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
export class TrayPage {
  protected readonly watches = inject(WatchesStore);
  protected readonly widgets = inject(TrayWidgetsStore);
  private readonly backend = inject(BACKEND);
  protected readonly unseen = computed(() => this.watches.unseen().length);
  /** What the last button press that failed said; cleared by the next press. */
  protected readonly actionError = signal<string | null>(null);
  protected readonly platform: SuTrayPlatform = (() => {
    const p = platformFromUserAgent(navigator.userAgent);
    return p === 'windows' ? 'windows' : p === 'linux' ? 'linux' : 'macos';
  })();

  constructor() {
    this.watches.start();
    this.widgets.start();
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

  protected openMain(): void {
    this.perform({ type: 'openSpotter' });
  }

  /** "Customize widgets…": Settings (the chooser is its page, ADR 0035 section 5). */
  protected customize(): void {
    this.perform({ type: 'openSettings' });
  }

  protected quit(): void {
    this.attempt(this.backend.quitApp());
  }

  /** Runs what a widget asked for through the command that does it; the engine decides the rest. */
  protected perform(action: WidgetActionId): void {
    switch (action.type) {
      case 'openSpotter':
        return this.show(null);
      case 'openWatch':
        return this.show(`/watches/${action.id}`);
      case 'openWatches':
        return this.show('/watches');
      case 'addWatch':
        return this.show('/watches/new');
      case 'openSettings':
        return this.show('/settings');
      case 'retryCheck':
        return this.attempt(this.backend.checkNow(action.id));
      case 'checkAll':
        return this.attempt(this.backend.checkNow(null));
      case 'resumeNotifications':
        return this.attempt(this.backend.pauseNotifications(null));
    }
  }

  /** Shows a page of the main window, then puts the popup away. */
  private show(route: string | null): void {
    this.attempt(this.backend.showMainWindow(route).then(() => this.close()));
  }

  private attempt(work: Promise<unknown>): void {
    this.actionError.set(null);
    work.catch((e: unknown) => this.actionError.set(toCommandError(e).message));
  }
}
