import { InjectionToken } from '@angular/core';

/**
 * What a dialog window shows. Rust loads the app's dialog route (`index.html#/dialog`, `paddock_dialogs::Dialogs::route`)
 * with `?id=<dialog>&host=<label>&label=<label>`, and the app boots the dialog window instead of its main window.
 */
export interface DialogRoute {
  /** The dialog's id (`pw.create-app`). */
  readonly id: string;
  /** The label of the window the dialog belongs to (`main`, `w-2`). */
  readonly host: string;
  /** This window's own label (`d-<host>-<n>`). */
  readonly label: string;
}

/** The hash path of the dialog route. */
export const DIALOG_ROUTE_PATH = '#/dialog';

const DIALOG_ID = /^[a-z0-9][a-z0-9.-]{0,63}$/;
const LABEL = /^[a-z0-9][a-z0-9-]{0,63}$/;

/** The window's location hash as a dialog route; `null` for any other window or anything malformed. */
export function parseDialogRoute(hash: string | null | undefined): DialogRoute | null {
  const match = /^#\/dialog\?(.*)$/.exec(hash ?? '');
  if (!match) return null;
  const params = new URLSearchParams(match[1]);
  const id = params.get('id') ?? '';
  const host = params.get('host') ?? '';
  const label = params.get('label') ?? '';
  if (!DIALOG_ID.test(id) || !LABEL.test(host) || !LABEL.test(label) || !label.startsWith('d-')) return null;
  return { id, host, label };
}

/** The hash that routes a window to a dialog (what Rust builds; tests and the browser build use it). */
export function dialogHash(route: DialogRoute, extra: Readonly<Record<string, string>> = {}): string {
  return `${DIALOG_ROUTE_PATH}?${new URLSearchParams({ id: route.id, host: route.host, label: route.label, ...extra }).toString()}`;
}

/** This window's dialog route, or `null` when it is not a dialog window. */
export const DIALOG_ROUTE = new InjectionToken<DialogRoute | null>('PADDOCK_DIALOG_ROUTE', {
  providedIn: 'root',
  factory: () => (typeof location === 'undefined' ? null : parseDialogRoute(location.hash)),
});
