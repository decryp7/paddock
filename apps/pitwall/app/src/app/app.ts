import { ChangeDetectionStrategy, Component, DOCUMENT, DestroyRef, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterOutlet } from '@angular/router';
import { filter, map } from 'rxjs';
import { isTauri } from './core/backend';
import { LiveStore } from './core/live.store';
import { ThemeService, isMac } from './core/theme';
import { WINDOW_CONTROL } from './core/window-control';
import { Sidebar } from './shell/sidebar';
import { ToastHost } from './shared/ui/toast-host';

/**
 * The window: a Linear-style sidebar and the routed screen. The tray popover (`#/tray`) is rendered alone in its own
 * small frameless window.
 */
@Component({
  selector: 'pw-root',
  imports: [RouterOutlet, Sidebar, ToastHost],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (tray()) {
      <router-outlet />
    } @else {
      <div class="titlebar" data-tauri-drag-region></div>
      <div class="shell">
        <pw-sidebar />
        <main id="main" tabindex="-1">
          <router-outlet />
        </main>
      </div>
      <a class="skip" href="#main" (click)="skip($event)">Skip to content</a>
    }
    <pw-toast-host />
  `,
  styles: `
    :host {
      display: block;
      height: 100vh;
    }
    .titlebar {
      position: fixed;
      inset: 0 0 auto 0;
      height: var(--titlebar-h);
      z-index: 50;
    }
    .shell {
      display: grid;
      grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
      height: 100%;
    }
    main {
      height: 100%;
      overflow: auto;
      background: var(--bg-content);
      border-left: 1px solid var(--border);
      outline: none;
      scrollbar-gutter: stable;
    }
    .skip {
      position: fixed;
      left: var(--sp-3);
      top: -40px;
      z-index: 200;
      padding: var(--sp-2) var(--sp-3);
      background: var(--accent);
      color: var(--text-on-accent);
      border-radius: var(--r-md);
      &:focus {
        top: var(--sp-3);
      }
    }
  `,
})
export class App {
  private readonly router = inject(Router);
  private readonly doc = inject(DOCUMENT);
  private readonly live = inject(LiveStore);
  protected readonly tray = toSignal(
    this.router.events.pipe(
      filter((e): e is NavigationEnd => e instanceof NavigationEnd),
      map((e) => e.urlAfterRedirects.startsWith('/tray')),
    ),
    { initialValue: this.doc.location?.hash.startsWith('#/tray') ?? false },
  );

  constructor() {
    const theme = inject(ThemeService);
    const nav = this.doc.defaultView?.navigator;
    const params = new URLSearchParams(this.doc.location?.search ?? '');
    // In the app the window has Mica (Windows) or vibrancy (macOS) behind it; `?translucent` previews it in a browser.
    theme.applyPlatform({ mac: isMac(nav?.userAgent ?? ''), translucent: isTauri() || params.has('translucent') });
    void this.live.load();
    this.live.connect();
    // The tray popover's "Open Pitwall" can ask the main window for a screen.
    const stop = inject(WINDOW_CONTROL).onNavigate((route) => void this.router.navigateByUrl(route));
    inject(DestroyRef).onDestroy(stop);
  }

  protected skip(e: Event): void {
    e.preventDefault();
    this.doc.getElementById('main')?.focus();
  }
}
