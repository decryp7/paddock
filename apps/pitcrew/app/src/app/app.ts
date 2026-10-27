import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { ThemeService } from './core/theme.service';
import { ConnectionBanner } from './layout/connection-banner';
import { Sidebar } from './layout/sidebar';
import { Toasts } from './layout/toasts';
import { CopyMenuHost } from './shared/ui/copy-menu';
import { StatusStore } from './state/status.store';

/** The main window: sidebar, connection banner, the routed page, and toasts. */
@Component({
  selector: 'pc-root',
  imports: [RouterOutlet, Sidebar, ConnectionBanner, Toasts, CopyMenuHost],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <a class="skip-link" href="#main" (click)="skip($event)">Skip to content</a>
    <div class="shell">
      <pc-sidebar class="sidebar" />
      <div class="content">
        <div class="titlebar" data-tauri-drag-region></div>
        <pc-connection-banner />
        <main id="main" tabindex="-1">
          <router-outlet />
        </main>
      </div>
    </div>
    <pc-toasts />
    <pc-copy-menu />
  `,
  styles: `
    :host {
      display: block;
      height: 100vh;
    }
    .shell {
      display: grid;
      grid-template-columns: var(--sidebar-width) 1fr;
      height: 100%;
    }
    .sidebar {
      position: relative;
      z-index: var(--z-sidebar);
      min-height: 0;
    }
    .content {
      display: flex;
      flex-direction: column;
      min-width: 0;
      min-height: 0;
      background: var(--pane);
    }
    /* macOS: the hidden title bar's strip stays draggable (0 px elsewhere). */
    .titlebar {
      flex: none;
      height: var(--titlebar-inset);
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
  // Start the theme and the status stream with the window.
  protected readonly theme = inject(ThemeService);
  protected readonly status = inject(StatusStore);

  protected skip(e: Event): void {
    e.preventDefault();
    document.getElementById('main')?.focus();
  }
}
