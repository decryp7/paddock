import { inject, InjectionToken, Injector, type InputSignalWithTransform, type Signal, type TemplateRef } from '@angular/core';
import { ContributionRegistry, type DialogDef, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import type { DialogAction } from './actions';

/**
 * What a dialog's content injects (`DIALOG_REF`). Most content never calls it: its declared actions answer and dismiss
 * (ADR 0026 §2). A content whose body holds the choice itself (a required choice's options) answers through it.
 */
export interface DialogRef<R = unknown> {
  /** Closes the dialog answered with `value`. */
  answer(value: R): void;
  /** Closes the dialog without an answer, unless the content refuses (`dialogCanClose`); a required choice ignores it. */
  dismiss(): void;
  /** Resolves when the dialog is gone, with its outcome. Never rejects. */
  readonly closed: Promise<DialogOutcome<R>>;
  /**
   * Sets the dialog's title (the OS window's title bar, or the in-page header, and the accessible name) to what the content
   * knows only after it loaded: "Delete 3 runners?" for a decision about three runners. Plain text, never markup.
   */
  setTitle?(title: string): void;
  /** Asks the frame to measure the content again (it changed what it shows without the frame seeing a resize). */
  requestFit?(): void;
}

/** What the opener of a dialog holds: its outcome, and a way to ask it to close (the dialog may refuse). */
export interface DialogHandle<R = unknown> {
  readonly closed: Promise<DialogOutcome<R>>;
  /** Asks the dialog to close without an answer, as its own dismiss action would. */
  dismiss(): void;
}

export const DIALOG_REF = new InjectionToken<DialogRef>('PADDOCK_DIALOG_REF');

/** The dialog's input as given to `open` (plain data; it crosses to another window). Each key also sets the component input of that name. */
export const DIALOG_INPUT = new InjectionToken<Readonly<Record<string, unknown>>>('PADDOCK_DIALOG_INPUT');

/** What a dialog component may declare about itself (ADR 0026 §2). */
export interface DialogContent<R = unknown> {
  /** Asked before the dialog is dismissed by any route; `false` keeps it open (unsaved edits). Runs in the dialog's own window. */
  dialogCanClose?(injector: Injector): boolean;
  /** The footer's buttons: the frame lays them out (order per OS, Enter, focus, busy, rejected). */
  readonly dialogActions?: Signal<readonly DialogAction<R>[]>;
  /** The footer's leading status line (a tool's "3 layouts"). */
  readonly dialogStatus?: Signal<string | null>;
  /** The body's declared minimum height, px (default two lines): with the header and footer, the window's OS minimum. */
  readonly dialogMinBodyHeight?: number;
  /** A header above the body that does not scroll (a stepper, a toolbar). */
  readonly dialogHeader?: Signal<TemplateRef<unknown> | null>;
}

/** The answer type of dialog `D` (its outcome is `DialogOutcome<DialogResultOf<D>>`). */
export type DialogResultOf<D> = D extends DialogDef<string, infer R, string> ? R : unknown;

/** The inputs a content component takes, from its `input()` signals: `{ current?: string }` for `current = input('')`. */
export type DialogInputsOf<C> = {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  readonly [K in keyof C as C[K] extends InputSignalWithTransform<any, any> ? K : never]?: C[K] extends InputSignalWithTransform<any, infer W> ? W : never;
};

/** What `open` takes as inputs for dialog `D`: its component's inputs when its `load` names the component, else any plain record. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type DialogInputsOfDef<D> = D extends DialogDef<string, any, string, infer C> ? (unknown extends C ? Readonly<Record<string, unknown>> : DialogInputsOf<C>) : Readonly<Record<string, unknown>>;

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
  /** Resolves when the dialog is gone, with its outcome. Never rejects. */
  readonly closed: Promise<DialogOutcome<unknown>>;
  /**
   * Resolves when the dialog is on screen: at once in the page, when the OS window is shown (the `shown` event) for a
   * window. A windowed modal blocks its parent from then on (the scrim over a macOS parent appears with the window, not
   * before). Never rejects; absent means shown at once.
   */
  readonly shown?: Promise<void>;
  /** False once the dialog is gone, before `closed` has been observed. */
  isOpen(): boolean;
  /** Brings the dialog to the front. */
  focus(): void;
  /** Asks the dialog to close without an answer; it may refuse (unsaved edits, a required choice, busy work). */
  dismiss(): void;
}

/** Shows dialogs: as OS windows (desktop) or in the page (browser build, e2e, a window that could not be opened). */
export abstract class DialogPresenter {
  /** Dialogs are OS windows: a modal one then disables its parent (natively, or by the host's scrim on macOS). */
  abstract readonly windowed: boolean;
  abstract show(request: DialogShowRequest): DialogSession;
}
