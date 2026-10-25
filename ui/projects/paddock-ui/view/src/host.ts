import { InjectionToken, type Type } from '@angular/core';
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

/** What a picker resolves: the engine's reference (never the path or the secret) and the name the user sees for it. */
export interface ViewPicked<T> {
  /** The `GrantRef` or `SecretRef` the form sends. */
  readonly value: T;
  /** What the form shows for the choice: a path (the view cuts it in the middle and shows the full value as a tooltip) or a secret's name. */
  readonly name: string;
}

/**
 * What a view needs from the app around it. The view only draws and reports; everything that asks the user or leaves the page is the
 * host's, so it can be an OS window (rule 17), a native picker or a grant dialog:
 *
 * - `confirm`: asks the action's question (a modal confirmation window) and resolves `true` to go on.
 * - `openInBrowser`: confirms the address with the user and opens it in the OS browser; the view never opens it itself.
 * - `pickPath`: the host's file or folder dialog; resolves the `GrantRef` (the engine's grant for the chosen path) with the path to
 *   show, or `null` when cancelled.
 * - `pickSecret`: chooses a stored secret; resolves the `SecretRef`, never the secret itself, with the secret's name, or `null`.
 *
 * A method may reject with an `Error`: the view shows its message inline.
 */
export interface ViewHost {
  confirm(request: ViewConfirmRequest): Promise<boolean>;
  openInBrowser(request: ViewOpenRequest): Promise<void>;
  pickPath(request: ViewPickRequest): Promise<ViewPicked<GrantRef> | null>;
  pickSecret(request: ViewPickRequest): Promise<ViewPicked<SecretRef> | null>;
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

/**
 * The app's own "needs local AI" component, when it has the window's Pitwall state (a wrapper over `<pd-needs-ai>` with a `feature`
 * input, which handles its own `(step)`). A `needsAi` node draws it instead of the plugin's reason, so the repair is the
 * app's (rule 19) and the state is the one the rest of the window shows. `null` (the default): the node draws the plugin's reason.
 */
export const VIEW_NEEDS_AI = new InjectionToken<Type<unknown> | null>('PADDOCK_VIEW_NEEDS_AI', { providedIn: 'root', factory: () => null });

/**
 * The app's "Open AI settings" (rule 19): what the plugin-less fallback card of a `needsAi` node offers as its only action when the
 * host has no Pitwall state (no {@link VIEW_NEEDS_AI}). It is the app's own repair, never routed through the plugin: the plugin's
 * `setup` action of a `needsAi` node is not sent to it. `null` (the default): the card shows the plugin's reason and a plain line
 * that where to set up local AI is up to the app, with no button.
 */
export const VIEW_OPEN_AI_SETTINGS = new InjectionToken<(() => void) | null>('PADDOCK_VIEW_OPEN_AI_SETTINGS', { providedIn: 'root', factory: () => null });
