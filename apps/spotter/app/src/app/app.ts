import { ChangeDetectionStrategy, Component, DestroyRef, inject } from '@angular/core';
import { Router, RouterOutlet } from '@angular/router';
import { BACKEND } from './core/backend';
import { ThemeService } from './core/theme.service';
import { ConnectionBanner } from './layout/connection-banner';
import { Operations } from './layout/operations';
import { Sidebar } from './layout/sidebar';
import { Toasts } from './layout/toasts';
import { CopyMenuHost } from './shared/ui/copy-menu';
import { SettingsStore } from './state/settings.store';
import { WatchesStore } from './state/watches.store';

/** Is this window the tray popup? Decided once: that window never navigates elsewhere. */
export function isTrayWindow(hash: string): boolean {
  return /^#\/tray(\/|$|\?)/.test(hash);
}

/** The main window (sidebar, banner, routed page, toasts) or, for the tray window, just the routed popup. */
@Component({
  selector: 'sp-root',
  imports: [RouterOutlet, Sidebar, ConnectionBanner, Operations, Toasts, CopyMenuHost],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (tray) {
      <router-outlet />
    } @else {
      <a class="skip-link" href="#main" (click)="skip($event)">Skip to content</a>
      <div class="shell">
        <sp-sidebar class="sidebar" />
        <div class="content">
          <div class="titlebar" data-tauri-drag-region></div>
          <sp-connection-banner />
          <main id="main" tabindex="-1">
            <router-outlet />
          </main>
        </div>
      </div>
      <sp-operations />
      <sp-toasts />
    }
    <sp-copy-menu />
  `,
  styles: `
    :host {
      display: block;
      height: 100vh;
    }
    .shell {
      display: grid;
      grid-template-columns: var(--pd-sidebar-width) 1fr;
      height: 100%;
    }
    .sidebar {
      position: relative;
      z-index: var(--pd-z-sidebar);
      min-height: 0;
    }
    .content {
      display: flex;
      flex-direction: column;
      min-width: 0;
      min-height: 0;
      background: var(--pd-bg);
    }
    .titlebar {
      flex: none;
      height: var(--sp-titlebar-inset);
    }
    main {
      flex: 1;
      min-height: 0;
      overflow: auto;
    }
    main:focus {
      outline: none;
    }
  `,
})
export class App {
  protected readonly tray = isTrayWindow(globalThis.location?.hash ?? '');
  private readonly backend = inject(BACKEND);
  private readonly router = inject(Router);

  constructor() {
    inject(ThemeService);
    if (this.tray) {
      document.documentElement.classList.add('pd-tray');
      return;
    }
    // Everything below starts in the background: the first paint never waits for the backend.
    inject(WatchesStore).start();
    void inject(SettingsStore).loadSettings();
    let unlisten: (() => void) | null = null;
    let closed = false;
    this.backend.onNavigate((route) => this.show(route)).then(
      (fn) => (closed ? fn() : (unlisten = fn)),
      () => undefined,
    );
    inject(DestroyRef).onDestroy(() => {
      closed = true;
      unlisten?.();
    });
  }

  private show(route: string): void {
    if (/^\/(watches(\/[\w-]+)?|settings)?$/.test(route)) void this.router.navigateByUrl(route);
  }

  protected skip(e: Event): void {
    e.preventDefault();
    document.getElementById('main')?.focus();
  }
}
