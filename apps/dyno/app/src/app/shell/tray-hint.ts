import { effect, inject, provideAppInitializer } from '@angular/core';
import { BACKEND, type DynoBackend } from '../core/backend';
import { PlatformService, type WindowPlatform } from '../core/platform';
import { Notifier } from '../shared/notifier';
import { WidgetLayouts } from '../state/widget-layouts';

/** Where the placement ids whose icons the user was told about are remembered (this webview only: a hint is no setting). */
export const TRAY_HINT_KEY = 'dyno.trayHintSeen';
/** What the hint says (sentence case; `^` is the chevron of the taskbar's hidden icons). */
export const TRAY_HINT_TEXT = "Drag Dyno's icons from ^ to the taskbar to keep them visible";
/** The notice's button. */
export const TRAY_HINT_ACTION = 'Open taskbar settings';

/** The placements that were not told about yet. */
export function unseenItems(ids: readonly string[], seen: readonly string[]): string[] {
  return ids.filter((id) => !seen.includes(id));
}

/**
 * Whether to show the hint: only on Windows (Windows 11 puts a new tray icon in the overflow, behind `^`), and only when
 * there is an icon the user was not told about yet: on the first start, and again when a new menu-bar item is added.
 */
export function trayHintDue(platform: WindowPlatform, ids: readonly string[], seen: readonly string[]): boolean {
  return platform === 'windows' && unseenItems(ids, seen).length > 0;
}

/** What the hint needs from the outside (so it is tested without a window). */
export interface TrayHintPorts {
  readonly platform: WindowPlatform;
  readonly readSeen: () => string[];
  readonly writeSeen: (ids: string[]) => void;
  readonly notify: (text: string, action: { label: string; run: () => void }) => void;
  readonly openSettings: () => void;
}

/** Shows the hint once for `ids` when it is due and remembers them; returns whether it was shown. */
export function showTrayHintOnce(ports: TrayHintPorts, ids: readonly string[]): boolean {
  const seen = ports.readSeen();
  if (!trayHintDue(ports.platform, ids, seen)) return false;
  ports.notify(TRAY_HINT_TEXT, { label: TRAY_HINT_ACTION, run: ports.openSettings });
  ports.writeSeen([...new Set([...seen, ...ids])]);
  return true;
}

/** The ids remembered in the webview's storage; storage may be blocked, then nothing is remembered. */
export function readSeen(storage: Pick<Storage, 'getItem'> | null = browserStorage()): string[] {
  try {
    const v: unknown = JSON.parse(storage?.getItem(TRAY_HINT_KEY) ?? '[]');
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

/** Remembers the ids; a blocked storage forgets them (the hint then shows again next start, which is harmless). */
export function writeSeen(ids: string[], storage: Pick<Storage, 'setItem'> | null = browserStorage()): void {
  try {
    storage?.setItem(TRAY_HINT_KEY, JSON.stringify(ids));
  } catch {
    // Private mode or blocked storage.
  }
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The ports of this window: the notice host, the shell's own taskbar settings command, the webview's storage. */
export function livePorts(platform: WindowPlatform, notifier: Notifier, backend: Pick<DynoBackend, 'openTaskbarSettings'>): TrayHintPorts {
  return {
    platform,
    readSeen: () => readSeen(),
    writeSeen: (ids) => writeSeen(ids),
    notify: (text, action) => notifier.show(text, 'info', action),
    openSettings: () => void backend.openTaskbarSettings().catch(() => undefined),
  };
}

/** The main window tells a Windows user once where the tray icons went, and again for each new menu-bar item. */
export function provideTrayHint() {
  return provideAppInitializer(() => {
    const platform = inject(PlatformService).platform;
    if (platform !== 'windows') return;
    const layouts = inject(WidgetLayouts);
    const ports = livePorts(platform, inject(Notifier), inject(BACKEND));
    void layouts.use('menuBar');
    effect(() => {
      const ids = layouts.menuBar()?.layout.placements.map((p) => p.id) ?? [];
      if (ids.length > 0) showTrayHintOnce(ports, ids);
    });
  });
}
