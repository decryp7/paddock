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
   * The ready handshake (ADR 0026 §4.2): the frame measured its content after the content and `document.fonts.ready`,
   * synchronously, without waiting for a frame (the 1.1-0 spike: a hidden window never runs `requestAnimationFrame` on a
   * sleeping Mac). The window system sizes the window and shows it. Sent once.
   */
  ready(measure: DialogMeasure): Promise<void>;
  /** A refit (ADR 0026 §4.5): the content's natural size, the theme, density or fonts changed, or the content asked. */
  fit(measure: DialogMeasure): Promise<void>;
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
}

/**
 * The dialog layer over Tauri: the `paddock_dialogs::tauri` commands and its events.
 *
 * The seam of package 1.1b (ADR 0026 §4): `ready` and `fit` are the only calls that size or show a dialog window.
 * Until Rust serves `dialog_ready`/`dialog_fit { content, minContent }`, both send the measured height to
 * `dialog_fit_height`, as before; the show mechanism per OS lives in Rust alone.
 */
export function tauriDialogIo(tauri: TauriBridge): DialogIo {
  return {
    open: (request) => tauri.invoke<DialogInfo>('dialog_open', { request }),
    take: () => tauri.invoke<DialogStart>('dialog_take_input'),
    close: (result) => tauri.invoke<void>('dialog_close', { result: result === undefined ? null : result }),
    listen: (event, handler) => tauri.listen(event, (e) => handler(e.payload)),
    requestClose: (label) => tauri.emitTo(label, DIALOG_EVENTS.closeRequested, null),
    ready: (m) => tauri.invoke<void>('dialog_fit_height', { content: m.content.height }),
    fit: (m) => tauri.invoke<void>('dialog_fit_height', { content: m.content.height }),
    ...(tauri.setWindowTitle ? { setTitle: (title: string) => tauri.setWindowTitle!(title) } : {}),
  };
}

/** Where the browser build leaves a dialog window's answer (`{"result": ...}`; e2e reads it). */
export const BROWSER_DIALOG_RESULT_KEY = 'paddock.dialog.browserResult';

/**
 * The browser build has no dialog windows (dialogs open in the page), but a dialog window's route can still be opened by
 * URL (`#/dialog?id=<id>&host=main&label=d-main-1&title=<title>&modality=<m>&dialogInput=<json>`), so e2e boots each
 * dialog with only its window's providers. The answer goes to `localStorage` and the tab asks to close. The ready
 * handshake sets `data-dialog-ready` on `<html>`; a measured height is published as `data-fitted-height` (and every height
 * asked for in order as `data-fit-history`), the minimum as `data-min-content` (`<width>x<height>`).
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
    setTitle: async (title) => {
      document.title = title;
    },
    ready: async (m) => {
      document.documentElement.dataset['dialogReady'] = 'true';
      publishMeasure(m);
    },
    fit: async (m) => publishMeasure(m),
  };
}

function publishMeasure(m: DialogMeasure): void {
  const data = document.documentElement.dataset;
  const px = String(m.content.height);
  data['fittedHeight'] = px;
  data['fitHistory'] = data['fitHistory'] ? `${data['fitHistory']},${px}` : px;
  data['minContent'] = `${m.minContent.width}x${m.minContent.height}`;
}

/** The window system of this window. Defaults to the browser build's; a desktop app provides `tauriDialogIo`. */
export const DIALOG_IO = new InjectionToken<DialogIo>('PADDOCK_DIALOG_IO', { providedIn: 'root', factory: () => browserDialogIo() });
