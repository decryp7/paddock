import { inject, Injectable } from '@angular/core';
import { DISMISSED, type DialogDef, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import { InPageDialogBackend } from './in-page';
import { instanceKey } from './kind';
import { ModalGate } from './modal-gate';
import { DIALOG_LOOKUP, DialogPresenter, type DialogHandle, type DialogInputsOfDef, type DialogResultOf, type DialogSession } from './types';

/** How to open a dialog; `I` is typed from the content component's `input()`s when the definition is passed. */
export interface DialogOpenOptions<I = Readonly<Record<string, unknown>>> {
  /** Plain data (it crosses to the dialog's window): each key sets the content component's input of that name. */
  readonly inputs?: I;
  /** The window title and accessible name; defaults to the dialog's declared title. */
  readonly title?: string;
}

/** Any defined dialog, whatever its id and result. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type AnyDialogDef = DialogDef<string, any, string, any>;

/**
 * Opens contributed dialogs (ADR 0009): as OS windows on the desktop, in the page in the browser build; the calls are
 * the same. Pass the definition for a typed result (`open(RENAME_DIALOG)` resolves to what its `result` declares) or an
 * id; the handle's `closed` resolves to the dialog's outcome (answered with a value, or dismissed). One window per dialog id (per input for an `instanced` one): opening it again focuses the open one and returns its
 * ref. In the page only modeless dialogs are single; modal ones stack, each blocking the last.
 */
@Injectable({ providedIn: 'root' })
export class DialogService {
  private readonly presenter = inject(DialogPresenter, { optional: true }) ?? inject(InPageDialogBackend);
  private readonly lookup = inject(DIALOG_LOOKUP);
  private readonly gate = inject(ModalGate);
  private readonly open_ = new Map<string, { readonly ref: DialogHandle; readonly session: DialogSession }>();

  /** A modal dialog is open over this window. */
  readonly blocked = this.gate.blocked;

  open<D extends AnyDialogDef>(dialog: D, options?: DialogOpenOptions<DialogInputsOfDef<D>>): DialogHandle<DialogResultOf<D>>;
  open<R = unknown>(id: string, options?: DialogOpenOptions): DialogHandle<R>;
  open(dialog: AnyDialogDef | string, options: DialogOpenOptions<object> = {}): DialogHandle<unknown> {
    const id = typeof dialog === 'string' ? dialog : dialog.id;
    // The registered definition is the contract (it is what Rust's copy was generated from), not the caller's object.
    const def = this.lookup(id);
    if (!def) {
      console.warn(`[paddock-dialogs] no dialog ${JSON.stringify(id)} is registered in this window`);
      return { closed: Promise.resolve(DISMISSED), dismiss: () => undefined };
    }
    const inputs = (options.inputs ?? {}) as Readonly<Record<string, unknown>>;
    const instance = def.instanced ? instanceKey(inputs) : undefined;
    const key = instance ? `${def.id}#${instance}` : def.id;
    const single = this.presenter.windowed || def.modality === 'modeless';
    const existing = single ? this.open_.get(key) : undefined;
    if (existing?.session.isOpen()) {
      existing.session.focus();
      return existing.ref;
    }
    // Focus goes back to the element that opened the dialog when it is gone (ADR 0026 §2).
    const opener = typeof document !== 'undefined' ? (document.activeElement as HTMLElement | null) : null;
    const session = this.presenter.show({ def, title: options.title ?? def.title.text, inputs, instance });
    const ref: DialogHandle = { closed: session.closed, dismiss: () => session.dismiss() };
    if (single) this.open_.set(key, { ref, session });
    const modal = def.modality === 'modal';
    // A windowed modal blocks its parent once its window is on screen, not when it is asked for; one closed before
    // that never entered.
    let ended = false;
    let entered = false;
    const enter = () => {
      if (ended || entered) return;
      entered = true;
      this.gate.enter();
    };
    if (modal) {
      if (this.presenter.windowed && session.shown) void session.shown.then(enter);
      else enter();
    }
    void session.closed.then(() => {
      ended = true;
      if (this.open_.get(key)?.ref === ref) this.open_.delete(key);
      if (entered) this.gate.leave();
      if (opener?.isConnected && opener !== document.body) opener.focus();
    });
    return ref;
  }

  /** `ShellPorts.openDialog`: opens dialog `id` with `inputs` and resolves with its outcome. */
  openById(id: string, inputs?: unknown): Promise<DialogOutcome<unknown>> {
    return this.open(id, { inputs: isPlainRecord(inputs) ? inputs : {} }).closed;
  }
}

function isPlainRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
