import { inject, Injectable } from '@angular/core';
import type { DialogDef } from '@decryp7/paddock-ui/shell';
import { InPageDialogBackend } from './in-page';
import { instanceKey } from './kind';
import { ModalGate } from './modal-gate';
import { DIALOG_LOOKUP, DialogPresenter, type DialogRef, type DialogResultOf, type DialogSession } from './types';

/** How to open a dialog. */
export interface DialogOpenOptions {
  /** Plain data (it crosses to the dialog's window): each key sets the content component's input of that name. */
  readonly inputs?: Readonly<Record<string, unknown>>;
  /** The window title and accessible name; defaults to the dialog's declared title. */
  readonly title?: string;
}

/** Any defined dialog, whatever its id and result. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type AnyDialogDef = DialogDef<string, any, string>;

/**
 * Opens contributed dialogs (ADR 0009): as OS windows on the desktop, in the page in the browser build; the calls are
 * the same. Pass the definition for a typed result (`open(RENAME_DIALOG)` resolves to what its `result` declares) or an
 * id. One window per dialog id (per input for an `instanced` one): opening it again focuses the open one and returns its
 * ref. In the page only modeless dialogs are single; modal ones stack, each blocking the last.
 */
@Injectable({ providedIn: 'root' })
export class DialogService {
  private readonly presenter = inject(DialogPresenter, { optional: true }) ?? inject(InPageDialogBackend);
  private readonly lookup = inject(DIALOG_LOOKUP);
  private readonly gate = inject(ModalGate);
  private readonly open_ = new Map<string, { readonly ref: DialogRef; readonly session: DialogSession }>();

  /** A modal dialog is open over this window. */
  readonly blocked = this.gate.blocked;

  open<D extends AnyDialogDef>(dialog: D, options?: DialogOpenOptions): DialogRef<DialogResultOf<D>>;
  open<R = unknown>(id: string, options?: DialogOpenOptions): DialogRef<R>;
  open(dialog: AnyDialogDef | string, options: DialogOpenOptions = {}): DialogRef<unknown> {
    const id = typeof dialog === 'string' ? dialog : dialog.id;
    // The registered definition is the contract (it is what Rust's copy was generated from), not the caller's object.
    const def = this.lookup(id);
    if (!def) {
      console.warn(`[paddock-dialogs] no dialog ${JSON.stringify(id)} is registered in this window`);
      return { closed: Promise.resolve(undefined), close: () => undefined };
    }
    const inputs = options.inputs ?? {};
    const instance = def.instanced ? instanceKey(inputs) : undefined;
    const key = instance ? `${def.id}#${instance}` : def.id;
    const single = this.presenter.windowed || def.modality === 'modeless';
    const existing = single ? this.open_.get(key) : undefined;
    if (existing?.session.isOpen()) {
      existing.session.focus();
      return existing.ref;
    }
    const session = this.presenter.show({ def, title: options.title ?? def.title.text, inputs, instance });
    const ref: DialogRef = { closed: session.closed, close: (r) => session.close(r) };
    if (single) this.open_.set(key, { ref, session });
    const modal = def.modality === 'modal';
    if (modal) this.gate.enter();
    void session.closed.then(() => {
      if (this.open_.get(key)?.ref === ref) this.open_.delete(key);
      if (modal) this.gate.leave();
    });
    return ref;
  }

  /** `ShellPorts.openDialog`: opens dialog `id` with `inputs` and resolves with its result. */
  openById(id: string, inputs?: unknown): Promise<unknown> {
    return this.open(id, { inputs: isPlainRecord(inputs) ? inputs : {} }).closed;
  }
}

function isPlainRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
