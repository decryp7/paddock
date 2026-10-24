import { Injectable, Injector, signal } from '@angular/core';
import type { DialogDef } from '@decryp7/paddock-ui/shell';
import { DialogPresenter, type DialogContent, type DialogRef, type DialogSession, type DialogShowRequest } from './types';

/** One dialog shown in the page. The host sets `content` and `injector` once it created the component. */
export interface InPageDialog {
  readonly n: number;
  readonly def: DialogDef;
  readonly title: string;
  readonly inputs: Readonly<Record<string, unknown>>;
  readonly ref: DialogRef;
  content: DialogContent | null;
  injector: Injector | null;
}

let next = 1;

/**
 * Shows dialogs in the page: the browser build (dev server, e2e) and a window that could not be opened (degrade, don't
 * fail). `pd-dialog-host` renders the stack: a modal dialog over a blocking backdrop, a modeless one as a floating panel
 * that leaves the page usable.
 */
@Injectable({ providedIn: 'root' })
export class InPageDialogBackend extends DialogPresenter {
  private readonly list = signal<readonly InPageDialog[]>([]);
  readonly windowed = false;
  /** The dialogs in the page, bottom to top. */
  readonly stack = this.list.asReadonly();

  show(request: DialogShowRequest): DialogSession {
    const n = next++;
    let resolve!: (r: unknown) => void;
    const closed = new Promise<unknown>((r) => (resolve = r));
    const isOpen = () => this.list().some((d) => d.n === n);
    const close = (result?: unknown) => {
      const entry = this.list().find((d) => d.n === n);
      if (!entry) return;
      if (entry.injector && entry.content?.dialogCanClose?.(entry.injector) === false) return;
      this.list.update((s) => s.filter((d) => d.n !== n));
      resolve(result);
    };
    const entry: InPageDialog = { n, def: request.def, title: request.title, inputs: request.inputs, ref: { closed, close }, content: null, injector: null };
    this.list.update((s) => [...s, entry]);
    return { closed, close, isOpen, focus: () => this.list.update((s) => raise(s, n)) };
  }
}

/** Brings dialog `n` to the front, but never over a modal opened after it (that one is answered first). */
export function raise(stack: readonly InPageDialog[], n: number): readonly InPageDialog[] {
  const at = stack.findIndex((d) => d.n === n);
  if (at < 0 || at === stack.length - 1 || stack.slice(at + 1).some((d) => d.def.modality === 'modal')) return stack;
  return [...stack.slice(0, at), ...stack.slice(at + 1), stack[at] as InPageDialog];
}
