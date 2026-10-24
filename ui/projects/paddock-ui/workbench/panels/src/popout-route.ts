import { InjectionToken } from '@angular/core';
import { parsePanelId, type PanelRef } from '@decryp7/paddock-ui/workbench';
import { validPanel } from './in-page';

/**
 * What a pop-out window shows: Rust loads the app's pop-out route (`index.html#/popout`, `paddock_panels::popout_url`) with
 * `?panel=<panel id>&host=<label>&label=<label>`, and the app boots the pop-out window instead of its main window.
 */
export interface PopoutRoute {
  /** The panel the window shows (`pn.code` or `pn.code#src/main.rs`): its view and instance key. */
  readonly panel: PanelRef;
  /** The label of the window the panel belongs to (`main`, `w-2`). */
  readonly host: string;
  /** This window's own label (`p-<host>-<n>`). */
  readonly label: string;
}

/** The hash path of the pop-out route. */
export const POPOUT_ROUTE_PATH = '#/popout';

const HOST = /^(?:main|w-\d+)$/;
const OWN = /^p-(?:main|w-\d+)-\d+$/;

/** The window's location hash as a pop-out route; `null` for any other window or anything malformed. */
export function parsePopoutRoute(hash: string | null | undefined): PopoutRoute | null {
  const match = /^#\/popout\?(.*)$/.exec(hash ?? '');
  if (!match) return null;
  const params = new URLSearchParams(match[1]);
  const panel = params.get('panel') ?? '';
  const host = params.get('host') ?? '';
  const label = params.get('label') ?? '';
  if (!validPanel(panel) || !HOST.test(host) || !OWN.test(label) || !label.startsWith(`p-${host}-`)) return null;
  return { panel: parsePanelId(panel), host, label };
}

/** The hash that routes a window to a pop-out (what Rust builds; tests and the browser build use it). */
export function popoutHash(route: PopoutRoute): string {
  return `${POPOUT_ROUTE_PATH}?${new URLSearchParams({ panel: route.panel.panel, host: route.host, label: route.label }).toString()}`;
}

/** This window's pop-out route, or `null` when it is not a pop-out. */
export const POPOUT_ROUTE = new InjectionToken<PopoutRoute | null>('PADDOCK_POPOUT_ROUTE', {
  providedIn: 'root',
  factory: () => (typeof location === 'undefined' ? null : parsePopoutRoute(location.hash)),
});
