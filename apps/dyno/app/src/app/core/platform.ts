import { DOCUMENT, Injectable, inject } from '@angular/core';

export type WindowPlatform = 'windows' | 'macos' | 'linux' | 'browser';
export type Vibrancy = 'mica' | 'vibrancy' | 'none';

/** Parses the shell's `data-platform` / `data-vibrancy` (set on <html> before Angular boots; absent in a browser). */
export function readPlatform(dataset: DOMStringMap): { platform: WindowPlatform; vibrancy: Vibrancy } {
  const p = dataset['platform'];
  const v = dataset['vibrancy'];
  return {
    platform: p === 'windows' || p === 'macos' || p === 'linux' ? p : 'browser',
    vibrancy: v === 'mica' || v === 'vibrancy' ? v : 'none',
  };
}

/** The native window chrome the UI adapts to (macOS inset traffic lights, translucent materials). */
@Injectable({ providedIn: 'root' })
export class PlatformService {
  private readonly info = readPlatform(inject(DOCUMENT).documentElement.dataset);
  readonly platform = this.info.platform;
  readonly vibrancy = this.info.vibrancy;
  /** macOS draws the traffic lights over the sidebar; the top strip is the drag region. */
  readonly overlayTitleBar = this.info.platform === 'macos';
  /** Shortcut modifier label. */
  readonly mod = this.info.platform === 'macos' ? '⌘' : 'Ctrl';
}
