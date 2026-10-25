/** Where the UI runs; the browser build has no window chrome. */
export type Platform = 'windows' | 'macos' | 'linux' | 'browser';

/**
 * Marks the platform on `<html>` for the window chrome: on macOS the title bar is hidden and the traffic lights sit
 * over the sidebar (`data-platform="macos"` leaves room for them); on Windows the webview is transparent over Mica
 * (`data-effects="mica"` makes the window background translucent while panes stay opaque). The browser build keeps
 * solid backgrounds.
 */
export function applyWindowChrome(root: HTMLElement, platform: Platform): void {
  root.dataset['platform'] = platform;
  if (platform === 'windows') root.dataset['effects'] = 'mica';
  else delete root.dataset['effects'];
}
