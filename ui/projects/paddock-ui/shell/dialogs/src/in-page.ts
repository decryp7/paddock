import { Injectable, Injector, signal, type Signal } from '@angular/core';
import { answered, DISMISSED, type DialogDef, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import { DialogPresenter, type DialogContent, type DialogRef, type DialogSession, type DialogShowRequest } from './types';

/** One dialog shown in the page. The view sets `content`, `injector` and `requestDismiss` once it created them. */
export interface InPageDialog {
  readonly n: number;
  readonly def: DialogDef;
  /** The title shown in the header; the content may change it (`DialogRef.setTitle`). */
  readonly title: Signal<string>;
  readonly inputs: Readonly<Record<string, unknown>>;
  /** What the content injects: answering or dismissing ends this dialog. */
  readonly ref: DialogRef;
  content: DialogContent | null;
  injector: Injector | null;
  /** The frame's dismissal (busy and required-choice rules); until the view is there, a plain dismissal. */
  requestDismiss: (() => void) | null;
  /** The frame's refit request, once the view is there. */
  requestFit: (() => void) | null;
  /** Applies a dismissal the frame decided on: `false` when the content kept the dialog open (unsaved edits). */
  readonly dismissNow: () => boolean;
}

let next = 1;

/**
 * Shows dialogs in the page: the browser build (dev server, e2e) and a window that could not be opened (degrade, don't
 * fail). `pd-dialog-host` renders the stack: a modal dialog over a blocking backdrop, a modeless one as a floating panel
 * that leaves the page usable. The same `DialogFrame` and rules apply as in a window (ADR 0026 §4.8).
 */
@Injectable({ providedIn: 'root' })
export class InPageDialogBackend extends DialogPresenter {
  private readonly list = signal<readonly InPageDialog[]>([]);
  readonly windowed = false;
  /** The dialogs in the page, bottom to top. */
  readonly stack = this.list.asReadonly();

  show(request: DialogShowRequest): DialogSession {
    const n = next++;
    let resolve!: (r: DialogOutcome<unknown>) => void;
    const closed = new Promise<DialogOutcome<unknown>>((r) => (resolve = r));
    const isOpen = () => this.list().some((d) => d.n === n);
    const end = (outcome: DialogOutcome<unknown>) => {
      if (!isOpen()) return;
      this.list.update((s) => s.filter((d) => d.n !== n));
      resolve(outcome);
    };
    /** A dismissal the frame decided on: the content may still keep the dialog open (unsaved edits). */
    const dismissNow = () => {
      if (entry.injector && entry.content?.dialogCanClose?.(entry.injector) === false) return false;
      end(DISMISSED);
      return true;
    };
    const title = signal(request.title);
    const ref: DialogRef = {
      closed,
      answer: (value) => end(answered(value)),
      dismiss: () => (entry.requestDismiss ? entry.requestDismiss() : void dismissNow()),
      setTitle: (text) => {
        if (text.trim()) title.set(text.trim());
      },
      requestFit: () => entry.requestFit?.(),
    };
    const entry: InPageDialog = {
      n,
      def: request.def,
      title: title.asReadonly(),
      inputs: request.inputs,
      ref,
      content: null,
      injector: null,
      requestDismiss: null,
      requestFit: null,
      dismissNow,
    };
    this.list.update((s) => [...s, entry]);
    return { closed, isOpen, dismiss: () => ref.dismiss(), focus: () => this.list.update((s) => raise(s, n)) };
  }
}

/** Brings dialog `n` to the front, but never over a modal opened after it (that one is answered first). */
export function raise(stack: readonly InPageDialog[], n: number): readonly InPageDialog[] {
  const at = stack.findIndex((d) => d.n === n);
  if (at < 0 || at === stack.length - 1 || stack.slice(at + 1).some((d) => d.def.modality === 'modal')) return stack;
  return [...stack.slice(0, at), ...stack.slice(at + 1), stack[at] as InPageDialog];
}
