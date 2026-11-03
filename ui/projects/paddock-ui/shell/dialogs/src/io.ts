import { InjectionToken } from '@angular/core';
import { DIALOG_EVENTS, type DialogInfo, type DialogRequest, type DialogSize, type DialogStart } from '@decryp7/paddock-ui/shell';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * What the dialog frame measured, the dialog's own inner logical size (ADR 0026 §4.2): `content` is what it needs at its
 * declared width (header, body at its natural height, footer), `minContent` the least it can be (header, footer, the
 * body's declared minimum; the footer's buttons on one line), which the window system makes the OS minimum.
 */
export interface DialogMeasure {
  readonly content: DialogSize;
  readonly minContent: DialogSize;
  /**
   * The page's `innerHeight` when it measured, CSS px: Rust adds what the window's inner height has beyond it (the spike
   * saw a viewport 32 px short of the window on a Mac, cutting the footer off).
   */
  readonly viewportHeight?: number;
  /**
   * The page's `window.devicePixelRatio` when it measured. Rust converts CSS px to logical px with the webview zoom,
   * `devicePixelRatio` over the window's scale factor (1 when absent). Finite, 0.25 to 8.
   */
  readonly devicePixelRatio?: number;
}

/**
 * Everything the dialog layer needs from the window system, in one seam (rule: code against abstractions). The desktop
 * build uses `tauriDialogIo` over the app's Tauri API; the browser build (dev server, e2e) uses `browserDialogIo`;
 * specs use `FakeDialogIo` from `shell/testing`. The wire types are generated from `paddock-dialogs`.
 */
export interface DialogIo {
  /** `dialog_open`: a window for the dialog, owned by this one (or the one it has open already, focused). */
  open(request: DialogRequest): Promise<DialogInfo>;
  /** `dialog_take_input`: this dialog window's start data. */
  take(): Promise<DialogStart>;
  /** `dialog_close`: answers the opener with `result` (none: dismissed) and ends this window. */
  close(result: unknown): Promise<void>;
  /** Listens to an event sent to this window. */
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
  /** Asks the dialog window `label` to close as if its Close/Cancel was pressed (it may refuse). */
  requestClose(label: string): Promise<void>;
  /**
   * Points the open dialog window `label` at new inputs (Settings at a section): a native event to that window, which reaches
   * another webview where a `BroadcastChannel` may not. Optional: a window system without it leaves the window where it is.
   */
  sendInputs?(label: string, dialogId: string, inputs: Readonly<Record<string, unknown>>): Promise<void>;
  /**
   * A line for the app's log (rule 56: what the dialog layer did across windows, never user content). Optional: without it
   * the line goes to the console only. Never throws and never waits.
   */
  log?(level: 'info' | 'warn', message: string): void;
  /**
   * One measure of the page (ADR 0043 §2, `dialog_measure`): the frame measured its content, synchronously where the window
   * is still hidden (the 1.1-0 spike: a hidden window never runs `requestAnimationFrame`). `(epoch, seq)` order it: a lower
   * `seq` of the same `epoch` is ignored, a new `epoch` (a reloaded page) restarts the order. Rust sizes the window.
   */
  measure(epoch: number, seq: number, measure: DialogMeasure): Promise<void>;
  /** The page says measure `seq` of `epoch` is the one to show the window at (`dialog_ready`); it carries no size. */
  ready(epoch: number, seq: number): Promise<void>;
  /** Sets this dialog window's title bar text (a title the content knows only at run time). Optional: a window system without it keeps the title it was opened with. */
  setTitle?(title: string): Promise<void>;
}

/** What `tauriDialogIo` needs of `@tauri-apps/api` (the app passes it in, so the package does not depend on Tauri). */
export interface TauriBridge {
  /** `invoke` from `@tauri-apps/api/core`. */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** `getCurrentWebviewWindow().listen` (events sent to this window only). */
  listen(event: string, handler: (event: { payload: unknown }) => void): Promise<Unlisten>;
  /** `emitTo` from `@tauri-apps/api/event`. */
  emitTo(target: string, event: string, payload?: unknown): Promise<void>;
  /**
   * `getCurrentWebviewWindow().setTitle` (needs the `core:window:allow-set-title` permission in the dialog windows'
   * capability). Optional: without it a runtime title changes the page's title and heading but not the OS title bar.
   */
  setWindowTitle?(title: string): Promise<void>;
  /** `log_from_webview` (the window's contract must name it). Optional: without it the dialog layer logs to the console only. */
  log?(level: 'info' | 'warn', message: string): Promise<unknown>;
}

/**
 * The dialog layer over Tauri: the `paddock_dialogs::tauri` commands and its events.
 *
 * The seam of ADR 0043: `measure` and `ready` are the only calls that size or show a dialog window. `measure` sends the
 * measure (`dialog_measure { epoch, seq, measure }`, the wire type `DialogMeasureWire`); Rust makes `minContent` the OS
 * minimum and sizes the window; `ready { epoch, seq }` names the measure to show at. The show mechanism per OS lives in Rust alone.
 */
export function tauriDialogIo(tauri: TauriBridge): DialogIo {
  return {
    open: (request) => tauri.invoke<DialogInfo>('dialog_open', { request }),
    take: () => tauri.invoke<DialogStart>('dialog_take_input'),
    close: (result) => tauri.invoke<void>('dialog_close', { result: result === undefined ? null : result }),
    listen: (event, handler) => tauri.listen(event, (e) => handler(e.payload)),
    requestClose: (label) => tauri.emitTo(label, DIALOG_EVENTS.closeRequested, null),
    sendInputs: (label, dialogId, inputs) => tauri.emitTo(label, DIALOG_EVENTS.inputs, { dialogId, inputs }),
    measure: (epoch, seq, measure) => tauri.invoke<void>('dialog_measure', { epoch, seq, measure }),
    ready: (epoch, seq) => tauri.invoke<void>('dialog_ready', { epoch, seq }),
    ...(tauri.setWindowTitle ? { setTitle: (title: string) => tauri.setWindowTitle!(title) } : {}),
    ...(tauri.log ? { log: (level: 'info' | 'warn', message: string) => void tauri.log!(level, message).catch(() => undefined) } : {}),
  };
}

/** Where the browser build leaves a dialog window's answer (`{"result": ...}`; e2e reads it). */
export const BROWSER_DIALOG_RESULT_KEY = 'paddock.dialog.browserResult';

/**
 * The browser build has no dialog windows (dialogs open in the page), but a dialog window's route can still be opened by
 * URL (`#/dialog?id=<id>&host=main&label=d-main-1&title=<title>&modality=<m>&dialogInput=<json>`), so e2e boots each
 * dialog with only its window's providers. The answer goes to `localStorage` and the tab asks to close. The ready
 * handshake sets `data-dialog-ready` on `<html>`; a measured height is published as `data-fitted-height` (and every height
 * asked for in order as `data-fit-history`), the width as `data-content-width`, the minimum as `data-min-content` (`<width>x<height>`).
 */
export function browserDialogIo(loc: Pick<Location, 'hash'> = location): DialogIo {
  const params = () => new URLSearchParams(loc.hash.split('?')[1] ?? '');
  return {
    open: () => Promise.reject(new Error('dialogs open in the page in the browser build')),
    take: async () => {
      const p = params();
      let input: unknown = {};
      try {
        input = JSON.parse(p.get('dialogInput') ?? '{}');
      } catch {
        /* no input */
      }
      const id = p.get('id') ?? '';
      return {
        dialogId: id,
        title: p.get('title') ?? id,
        kind: 'form',
        modality: p.get('modality') === 'modal' ? 'modal' : 'modeless',
        requiredChoice: false,
        input,
        epoch: 1,
      };
    },
    close: async (result) => {
      try {
        localStorage.setItem(BROWSER_DIALOG_RESULT_KEY, JSON.stringify({ result: result ?? null }));
      } catch {
        /* a convenience only */
      }
      window.close();
    },
    listen: async () => () => undefined,
    requestClose: async () => undefined,
    sendInputs: async () => undefined,
    setTitle: async (title) => {
      document.title = title;
    },
    measure: async (_epoch, seq, m) => {
      document.documentElement.dataset['fitSeq'] = String(seq);
      publishMeasure(m);
    },
    ready: async () => {
      document.documentElement.dataset['dialogReady'] = 'true';
    },
  };
}

function publishMeasure(m: DialogMeasure): void {
  const data = document.documentElement.dataset;
  const px = String(m.content.height);
  data['fittedHeight'] = px;
  data['fitHistory'] = data['fitHistory'] ? `${data['fitHistory']},${px}` : px;
  data['minContent'] = `${m.minContent.width}x${m.minContent.height}`;
  data['contentWidth'] = String(m.content.width);
}

/** The window system of this window. Defaults to the browser build's; a desktop app provides `tauriDialogIo`. */
export const DIALOG_IO = new InjectionToken<DialogIo>('PADDOCK_DIALOG_IO', { providedIn: 'root', factory: () => browserDialogIo() });
