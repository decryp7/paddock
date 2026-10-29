import { ChangeDetectionStrategy, Component, DOCUMENT, DestroyRef, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterOutlet } from '@angular/router';
import { filter, map } from 'rxjs';
import { JobsPanel } from './features/jobs/jobs-panel';
import { QuitPrompt } from './features/quit/quit-prompt';
import { ServiceNotices } from './features/service/service-notices';
import { ServiceConsentPrompt } from './features/service/service-consent';
import { ServiceStore } from './features/service/service.store';
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
  imports: [RouterOutlet, Sidebar, ToastHost, ServiceNotices, JobsPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (tray()) {
      <router-outlet />
    } @else {
      <div class="titlebar" data-tauri-drag-region></div>
      <div class="shell">
        <pw-sidebar />
        <main id="main" tabindex="-1">
          <pw-service-notices class="notices" />
          <pw-jobs-panel class="notices" />
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
      height: var(--pw-titlebar-h);
      z-index: 50;
    }
    .shell {
      display: grid;
      grid-template-columns: var(--pd-sidebar-width) minmax(0, 1fr);
      height: 100%;
    }
    main {
      height: 100%;
      overflow: auto;
      background: var(--pd-bg);
      border-left: 1px solid var(--pd-hairline);
      outline: none;
      scrollbar-gutter: stable;
    }
    .notices {
      padding: var(--pd-space-3) var(--pd-space-4) 0;
    }
    .skip {
      position: fixed;
      left: var(--pd-space-3);
      top: -40px;
      z-index: 200;
      padding: var(--pd-space-2) var(--pd-space-3);
      background: var(--pd-accent);
      color: var(--pd-on-accent);
      border-radius: var(--pd-radius-control);
      &:focus {
        top: var(--pd-space-3);
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
    theme.applyPlatform({ mac: isMac(nav?.userAgent ?? '') });
    void this.live.load();
    this.live.connect();
    // The service question belongs to the main window only, and never delays the first paint: it starts in the background.
    if (!this.doc.location?.hash.startsWith('#/tray')) {
      const service = inject(ServiceStore);
      const prompt = inject(ServiceConsentPrompt);
      void service.refresh();
      effect(() => {
        if (service.askConsent()) void prompt.ask();
      });
      // "When you quit Pitwall: ask each time": the app asks this window to show the question.
      const stopQuit = inject(QuitPrompt).watch();
      inject(DestroyRef).onDestroy(stopQuit);
    }
    // The tray popover's "Open Pitwall" can ask the main window for a screen.
    const stop = inject(WINDOW_CONTROL).onNavigate((route) => void this.router.navigateByUrl(route));
    inject(DestroyRef).onDestroy(stop);
  }

  protected skip(e: Event): void {
    e.preventDefault();
    this.doc.getElementById('main')?.focus();
  }
}
