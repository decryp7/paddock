import { inject, Injectable } from '@angular/core';
import { DIALOG_EVENTS, type DialogClosed, type DialogRequest, type DialogResult } from '@decryp7/paddock-ui/shell';
import { isRememberedPlace, loadDialogBounds, rememberDialogBounds } from './bounds';
import { InPageDialogBackend } from './in-page';
import { DIALOG_IO } from './io';
import { raiseToKindMinimum } from './kind';
import { errorText, isRecord } from './mount';
import { DialogPresenter, type DialogSession, type DialogShowRequest } from './types';

/** A result event of the dialog layer; `null` when malformed. */
export function parseResult(p: unknown): DialogResult | null {
  return isRecord(p) && typeof p['label'] === 'string' && typeof p['dialogId'] === 'string' ? { label: p['label'], dialogId: p['dialogId'], result: p['result'] } : null;
}

/** A closed event of the dialog layer; `null` when malformed. */
export function parseClosed(p: unknown): DialogClosed | null {
  if (!isRecord(p) || typeof p['label'] !== 'string' || typeof p['dialogId'] !== 'string') return null;
  const r = p['remembered'];
  return { label: p['label'], dialogId: p['dialogId'], remembered: isRememberedPlace(r) ? { dx: r.dx, dy: r.dy, width: r.width, height: r.height } : null };
}

/** What a dialog window is opened with: its id, title and input, and what was remembered. Its size is the Rust contract's. */
export function windowRequest(request: DialogShowRequest, remembered = loadDialogBounds()[request.def.id]): DialogRequest {
  const def = request.def;
  return {
    dialogId: def.id,
    instance: request.instance,
    title: request.title,
    input: request.inputs,
    // Where it goes is Rust's decision (on the parent's monitor); the kind's floor applies to a remembered size.
    remembered: remembered ? raiseToKindMinimum(def.kind, remembered) : undefined,
  };
}

interface Pending {
  readonly dialogId: string;
  readonly resolve: (r: unknown) => void;
  result: unknown;
}

/**
 * Dialogs as real OS windows (ADR 0009, rule 17): `dialog_open` asks Rust for a window owned by this one (on its
 * monitor, by its kind, at the size it had last time), the dialog answers with `paddock://dialog-result`, and
 * `paddock://dialog-closed` ends it (with its size and offset to remember). A window that cannot be opened is shown in
 * the page instead (degrade, don't fail).
 */
@Injectable({ providedIn: 'root' })
export class WindowDialogBackend extends DialogPresenter {
  private readonly io = inject(DIALOG_IO);
  private readonly inPage = inject(InPageDialogBackend);
  private readonly pending = new Map<string, Pending>();
  private listening: Promise<void> | null = null;
  readonly windowed = true;

  show(request: DialogShowRequest): DialogSession {
    let resolve!: (r: unknown) => void;
    const closed = new Promise<unknown>((r) => (resolve = r));
    let label: string | null = null;
    let open = true;
    let fallback: DialogSession | null = null;
    const sent = windowRequest(request);
    const opened = (async () => {
      try {
        await this.listen();
        const info = await this.io.open(sent);
        label = info.label;
        this.pending.set(info.label, { dialogId: request.def.id, resolve, result: undefined });
      } catch (e) {
        console.warn(`[paddock-dialogs] the window of dialog ${request.def.id} could not open; shown in the page: ${errorText(e)}`);
        fallback = this.inPage.show(request);
        void fallback.closed.then(resolve);
      }
    })();
    void closed.then(() => (open = false));
    return {
      closed,
      isOpen: () => open,
      // Opening it again focuses the window Rust has open for it.
      focus: () => void opened.then(() => (fallback ? fallback.focus() : label ? this.io.open(sent).then(() => undefined, () => undefined) : undefined)),
      close: (result) => void opened.then(() => (fallback ? fallback.close(result) : label ? this.io.requestClose(label).catch(() => undefined) : undefined)),
    };
  }

  private listen(): Promise<void> {
    this.listening ??= Promise.all([
      this.io.listen(DIALOG_EVENTS.result, (p) => {
        const e = parseResult(p);
        const entry = e && this.pending.get(e.label);
        if (e && entry) entry.result = e.result ?? undefined;
      }),
      this.io.listen(DIALOG_EVENTS.closed, (p) => {
        const e = parseClosed(p);
        const entry = e && this.pending.get(e.label);
        if (!e || !entry) return;
        this.pending.delete(e.label);
        if (e.remembered) rememberDialogBounds(entry.dialogId, e.remembered);
        entry.resolve(entry.result === null ? undefined : entry.result);
      }),
    ]).then(
      () => undefined,
      (e: unknown) => {
        // Retried on the next open; this one is shown in the page.
        this.listening = null;
        throw e;
      },
    );
    return this.listening;
  }
}
