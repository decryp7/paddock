import { InjectionToken } from '@angular/core';
import type { ActionRef, GrantRef, SecretRef } from '../generated/view';

/** What the host is asked when an action carries a `confirm` question. */
export interface ViewConfirmRequest {
  /** The plugin's question, as text. */
  readonly question: string;
  /** The action being confirmed (its label is the button of the host's confirmation). */
  readonly action: ActionRef;
  /** The action changes something (`ActionRef.writes`). */
  readonly writes: boolean;
}

/** What the host is asked to open in the OS browser (a `Link.openInBrowser`; the URL passed the web-address check). */
export interface ViewOpenRequest {
  readonly url: string;
  readonly label: string;
}

/** What the host is asked when the user chooses a path or a secret for a form field. */
export interface ViewPickRequest {
  /** The field's id. */
  readonly fieldId: string;
  /** The field's label (the picker's title). */
  readonly label: string;
  /** A path field wants a folder. */
  readonly folder?: boolean;
}

/**
 * What a view needs from the app around it. The view only draws and reports; everything that asks the user or leaves the page is the
 * host's, so it can be an OS window (rule 17), a native picker or a grant dialog:
 *
 * - `confirm`: asks the action's question (a modal confirmation window) and resolves `true` to go on.
 * - `openInBrowser`: confirms the address with the user and opens it in the OS browser; the view never opens it itself.
 * - `pickPath`: the host's file or folder dialog; resolves a `GrantRef` (the engine's grant for the chosen path), or `null` when cancelled.
 * - `pickSecret`: chooses a stored secret; resolves a `SecretRef`, never the secret itself, or `null`.
 *
 * A method may reject with an `Error`: the view shows its message inline.
 */
export interface ViewHost {
  confirm(request: ViewConfirmRequest): Promise<boolean>;
  openInBrowser(request: ViewOpenRequest): Promise<void>;
  pickPath(request: ViewPickRequest): Promise<GrantRef | null>;
  pickSecret(request: ViewPickRequest): Promise<SecretRef | null>;
}

const unavailable = (what: () => string) => (): Promise<never> =>
  Promise.reject(new Error($localize`:@@pd.view.host.missing:This app cannot ${what()}:what: from a view yet, so nothing was done.`));

/** The host of an app that provided none: every request is refused with a plain message, so nothing runs unconfirmed. */
export const NO_VIEW_HOST: ViewHost = {
  confirm: unavailable(() => $localize`:@@pd.view.host.confirm:ask for a confirmation`),
  openInBrowser: unavailable(() => $localize`:@@pd.view.host.open:open a web page`),
  pickPath: unavailable(() => $localize`:@@pd.view.host.path:choose a file or folder`),
  pickSecret: unavailable(() => $localize`:@@pd.view.host.secret:choose a secret`),
};

/** The app's {@link ViewHost}: `{ provide: VIEW_HOST, useValue: myHost }` in the window that shows views. */
export const VIEW_HOST = new InjectionToken<ViewHost>('PADDOCK_VIEW_HOST', { providedIn: 'root', factory: () => NO_VIEW_HOST });
