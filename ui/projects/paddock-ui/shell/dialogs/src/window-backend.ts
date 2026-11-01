import { inject, Injectable } from '@angular/core';
import { DIALOG_EVENTS, type DialogClosed, type DialogOutcome, type DialogRequest, type DialogResult } from '@decryp7/paddock-ui/shell';
import { decodeOutcome } from './outcome';
import { isRememberedPlace, loadDialogBounds, rememberDialogBounds } from './bounds';
import { InPageDialogBackend } from './in-page';
import { DIALOG_IO } from './io';
import { raiseToKindMinimum } from './kind';
import { errorText, isRecord } from './mount';
import { sendDialogInputs } from './retarget';
import { DialogPresenter, type DialogSession, type DialogShowRequest } from './types';

/** A result event of the dialog layer; `null` when malformed. */
export function parseResult(p: unknown): DialogResult | null {
  return isRecord(p) && typeof p['label'] === 'string' && typeof p['dialogId'] === 'string' ? { label: p['label'], dialogId: p['dialogId'], result: p['result'] } : null;
}

/** A shown event of the dialog layer; `null` when malformed. */
export function parseShown(p: unknown): { label: string; dialogId: string } | null {
  return isRecord(p) && typeof p['label'] === 'string' && typeof p['dialogId'] === 'string' ? { label: p['label'], dialogId: p['dialogId'] } : null;
}

/** A closed event of the dialog layer; `null` when malformed. */
export function parseClosed(p: unknown): DialogClosed | null {
  if (!isRecord(p) || typeof p['label'] !== 'string' || typeof p['dialogId'] !== 'string') return null;
  const r = p['remembered'];
  return { label: p['label'], dialogId: p['dialogId'], remembered: isRememberedPlace(r) ? { dx: r.dx, dy: r.dy, width: r.width, height: r.height } : null };
}

/** The app's resolved theme: its `data-theme`, else the system's colour scheme (a dialog window's background before its page paints). */
export function currentTheme(): 'light' | 'dark' {
  const set = document.documentElement.dataset['theme'];
  if (set === 'light' || set === 'dark') return set;
  return typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
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
    theme: currentTheme(),
  };
}

interface Pending {
  readonly dialogId: string;
  readonly resolve: (r: DialogOutcome<unknown>) => void;
  /** The window is on screen. */
  readonly shown: () => void;
  /** What the dialog closed with, on the wire (`null`: dismissed, also when Rust closed a page that did not answer). */
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
  /** Windows shown before `open` returned to this side (the event can overtake the reply). */
  private readonly shownEarly = new Set<string>();
  private listening: Promise<void> | null = null;
  readonly windowed = true;

  show(request: DialogShowRequest): DialogSession {
    let resolve!: (r: DialogOutcome<unknown>) => void;
    const closed = new Promise<DialogOutcome<unknown>>((r) => (resolve = r));
    let shown!: () => void;
    const onScreen = new Promise<void>((r) => (shown = r));
    let label: string | null = null;
    let open = true;
    let fallback: DialogSession | null = null;
    const sent = windowRequest(request);
    const opened = (async () => {
      try {
        await this.listen();
        const info = await this.io.open(sent);
        label = info.label;
        // A window another window opened is pointed at the new inputs (it only comes forward otherwise).
        if (info.existing && Object.keys(request.inputs).length > 0 && !request.def.instanced) {
          // This page's own dialogs hear it at once; the window is another webview, so it is told by a native event.
          sendDialogInputs(request.def.id, request.inputs);
          this.io.log?.('info', `[paddock-dialogs] re-targeting the open window ${info.label} of ${request.def.id} (opened by another window): ${Object.keys(request.inputs).join(',')}`);
          await this.io.sendInputs?.(info.label, request.def.id, request.inputs)?.catch((e: unknown) => console.warn(`[paddock-dialogs] the open window of ${request.def.id} could not be re-targeted: ${errorText(e)}`));
        }
        this.pending.set(info.label, { dialogId: request.def.id, resolve, shown, result: null });
        // An existing window that is on screen already will not announce itself again; a pending one (or a new one) does.
        if ((info.existing && !info.pending) || this.shownEarly.delete(info.label)) shown();
      } catch (e) {
        console.warn(`[paddock-dialogs] the window of dialog ${request.def.id} could not open; shown in the page: ${errorText(e)}`);
        fallback = this.inPage.show(request);
        shown();
        void fallback.closed.then(resolve);
      }
    })();
    void closed.then(() => {
      open = false;
      shown();
    });
    return {
      closed,
      shown: onScreen,
      isOpen: () => open,
      // Opening it again focuses the window Rust has open for it.
      focus: () =>
        void opened.then(() => {
          if (fallback) return fallback.focus();
          if (!label) return undefined;
          this.io.log?.('info', `[paddock-dialogs] focusing the open window ${label} of ${request.def.id} (this window opened it)`);
          return this.io.open(sent).then(() => undefined, () => undefined);
        }),
      dismiss: () => void opened.then(() => (fallback ? fallback.dismiss() : label ? this.io.requestClose(label).catch(() => undefined) : undefined)),
    };
  }

  private listen(): Promise<void> {
    this.listening ??= Promise.all([
      this.io.listen(DIALOG_EVENTS.result, (p) => {
        const e = parseResult(p);
        const entry = e && this.pending.get(e.label);
        if (e && entry) entry.result = e.result ?? null;
      }),
      this.io.listen(DIALOG_EVENTS.shown, (p) => {
        const e = parseShown(p);
        if (!e) return;
        const entry = this.pending.get(e.label);
        if (entry) entry.shown();
        else this.shownEarly.add(e.label);
      }),
      this.io.listen(DIALOG_EVENTS.closed, (p) => {
        const e = parseClosed(p);
        if (!e) return;
        this.shownEarly.delete(e.label);
        const entry = this.pending.get(e.label);
        if (!entry) return;
        this.pending.delete(e.label);
        if (e.remembered) rememberDialogBounds(entry.dialogId, e.remembered);
        entry.resolve(decodeOutcome(entry.result));
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
