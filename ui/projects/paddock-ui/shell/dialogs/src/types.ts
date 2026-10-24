import { inject, InjectionToken, Injector } from '@angular/core';
import { ContributionRegistry, type DialogDef } from '@decryp7/paddock-ui/shell';

/** What a dialog's content injects to answer: closing it resolves the opener's promise with the result. */
export interface DialogRef<R = unknown> {
  /** Closes the dialog with `result` (none: dismissed), unless the content refuses (`dialogCanClose`). */
  close(result?: R): void;
  /** Resolves when the dialog is gone, with its result (`undefined` when dismissed). Never rejects. */
  readonly closed: Promise<R | undefined>;
}

export const DIALOG_REF = new InjectionToken<DialogRef>('PADDOCK_DIALOG_REF');

/** The dialog's input as given to `open` (plain data; it crosses to another window). Each key also sets the component input of that name. */
export const DIALOG_INPUT = new InjectionToken<Readonly<Record<string, unknown>>>('PADDOCK_DIALOG_INPUT');

/** What a dialog component may declare about itself. */
export interface DialogContent {
  /** Asked before the dialog closes by any route; `false` keeps it open (unsaved edits). Runs in the dialog's own window. */
  dialogCanClose?(injector: Injector): boolean;
}

/** What the opener's promise of dialog `D` resolves to. */
export type DialogResultOf<D> = D extends DialogDef<string, infer R, string> ? R : unknown;

/** Finds a dialog's definition by id: the window's contribution registry, or the list a dialog window was booted with. */
export type DialogLookup = (id: string) => DialogDef | undefined;

export const DIALOG_LOOKUP = new InjectionToken<DialogLookup>('PADDOCK_DIALOG_LOOKUP', {
  providedIn: 'root',
  factory: () => {
    const registry = inject(ContributionRegistry);
    return (id) => registry.find<DialogDef>('dialogs', id);
  },
});

/** What a presenter needs to show a dialog. */
export interface DialogShowRequest {
  readonly def: DialogDef;
  /** The window title and accessible name. */
  readonly title: string;
  /** Structured-clonable: it crosses to another window. */
  readonly inputs: Readonly<Record<string, unknown>>;
  /** Tells apart the windows of an instanced dialog (letters and digits). */
  readonly instance?: string;
}

/** An open dialog, however it is shown. */
export interface DialogSession {
  /** Resolves when the dialog is gone, with its result (`undefined` when dismissed). Never rejects. */
  readonly closed: Promise<unknown>;
  /** False once the dialog is gone, before `closed` has been observed. */
  isOpen(): boolean;
  /** Brings the dialog to the front. */
  focus(): void;
  /** Asks the dialog to close with a result; it may refuse (unsaved edits). */
  close(result?: unknown): void;
}

/** Shows dialogs: as OS windows (desktop) or in the page (browser build, e2e, a window that could not be opened). */
export abstract class DialogPresenter {
  /** Dialogs are OS windows: a modal one then disables its parent (natively, or by the host's scrim on macOS). */
  abstract readonly windowed: boolean;
  abstract show(request: DialogShowRequest): DialogSession;
}
