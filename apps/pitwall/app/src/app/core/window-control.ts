import { InjectionToken } from '@angular/core';
import { isTauri } from './backend';

/** Window actions of the tray popover (not a server call, so not part of the backend). */
export interface WindowControl {
  /** Shows and focuses the main window, optionally on a route, and hides the popover. */
  openMain(route?: string): Promise<void>;
  /** In the main window: routes the tray asks for. Returns a function that stops listening. */
  onNavigate(handler: (route: string) => void): () => void;
}

/* v8 ignore start -- needs the Tauri webview (capabilities: core:window:allow-show, allow-set-focus, allow-hide). */
class TauriWindowControl implements WindowControl {
  async openMain(route = '/'): Promise<void> {
    const { Window, getCurrentWindow } = await import('@tauri-apps/api/window');
    const { emitTo } = await import('@tauri-apps/api/event');
    const main = await Window.getByLabel('main');
    if (main) {
      await main.show();
      await main.unminimize();
      await main.setFocus();
      await emitTo('main', 'pitwall://navigate', route);
    }
    await getCurrentWindow().hide();
  }

  onNavigate(handler: (route: string) => void): () => void {
    let stop: (() => void) | null = null;
    let stopped = false;
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<string>('pitwall://navigate', (e) => handler(e.payload)).then((u) => (stopped ? u() : (stop = u))),
    );
    return () => {
      stopped = true;
      stop?.();
    };
  }
}
/* v8 ignore stop */

/** In the browser build the "main window" is this tab: just navigate. */
export class BrowserWindowControl implements WindowControl {
  constructor(private readonly nav: (url: string) => void = (url) => (location.hash = url)) {}
  async openMain(route = '/'): Promise<void> {
    this.nav(`#${route}`);
  }

  onNavigate(): () => void {
    return () => undefined;
  }
}

export const WINDOW_CONTROL = new InjectionToken<WindowControl>('WindowControl', {
  providedIn: 'root',
  factory: () => (isTauri() ? new TauriWindowControl() : new BrowserWindowControl()),
});
