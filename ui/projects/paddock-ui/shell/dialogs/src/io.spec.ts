import { DIALOG_EVENTS } from '@decryp7/paddock-ui/shell';
import { BROWSER_DIALOG_RESULT_KEY, browserDialogIo, tauriDialogIo, type TauriBridge } from './io';

describe('tauriDialogIo', () => {
  it('calls the four commands of paddock_dialogs::tauri and its events', async () => {
    const calls: unknown[][] = [];
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const bridge: TauriBridge = {
      invoke: async <T>(cmd: string, args?: Record<string, unknown>) => (calls.push([cmd, args]), ({ label: 'd-main-1' }) as T),
      listen: async (event, h) => (calls.push(['listen', event]), (handler = h), () => undefined),
      emitTo: async (target, event, payload) => void calls.push(['emitTo', target, event, payload]),
    };
    const io = tauriDialogIo(bridge);
    await io.open({ dialogId: 'a.b', title: 'T' });
    await io.take();
    await io.close(undefined);
    await io.close({ ok: 1 });
    await io.ready({ content: { width: 400, height: 321 }, minContent: { width: 300, height: 150 } });
    await io.fit({ content: { width: 400, height: 330 }, minContent: { width: 300, height: 150 } });
    await io.requestClose('d-main-1');
    const got: unknown[] = [];
    await io.listen(DIALOG_EVENTS.result, (p) => got.push(p));
    handler!({ payload: 7 });
    expect(got).toEqual([7]);
    expect(calls).toEqual([
      ['dialog_open', { request: { dialogId: 'a.b', title: 'T' } }],
      ['dialog_take_input', undefined],
      ['dialog_close', { result: null }],
      ['dialog_close', { result: { ok: 1 } }],
      ['dialog_fit_height', { content: 321 }],
      ['dialog_fit_height', { content: 330 }],
      ['emitTo', 'd-main-1', DIALOG_EVENTS.closeRequested, null],
      ['listen', DIALOG_EVENTS.result],
    ]);
  });
});

describe('browserDialogIo', () => {
  afterEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset['fittedHeight'];
    delete document.documentElement.dataset['fitHistory'];
    delete document.documentElement.dataset['dialogReady'];
    delete document.documentElement.dataset['minContent'];
  });

  it('reads its start from the route and leaves the answer for e2e', async () => {
    const io = browserDialogIo({ hash: '#/dialog?id=a.b&host=main&label=d-main-1&title=About&modality=modal&dialogInput=%7B%22a%22%3A1%7D' });
    expect(await io.take()).toEqual({ dialogId: 'a.b', title: 'About', kind: 'form', modality: 'modal', requiredChoice: false, input: { a: 1 } });
    const close = vi.spyOn(window, 'close').mockImplementation(() => undefined);
    await io.close({ ok: 1 });
    expect(JSON.parse(localStorage.getItem(BROWSER_DIALOG_RESULT_KEY)!)).toEqual({ result: { ok: 1 } });
    await io.close(undefined);
    expect(localStorage.getItem(BROWSER_DIALOG_RESULT_KEY)).toBe('{"result":null}');
    expect(close).toHaveBeenCalledTimes(2);
    close.mockRestore();
    await expect(io.open({ dialogId: 'x', title: 't' })).rejects.toThrow(/in the page/);
    (await io.listen('e', () => undefined))();
    await io.requestClose('d-main-1');
    await io.ready({ content: { width: 400, height: 200 }, minContent: { width: 320, height: 120 } });
    await io.fit({ content: { width: 400, height: 260 }, minContent: { width: 330, height: 120 } });
    const data = document.documentElement.dataset;
    expect([data['dialogReady'], data['fittedHeight'], data['fitHistory'], data['minContent']]).toEqual(['true', '260', '200,260', '330x120']);
  });

  it('tolerates a route without input, a bad input and a full storage', async () => {
    expect(await browserDialogIo({ hash: '#/dialog' }).take()).toMatchObject({ dialogId: '', title: '', modality: 'modeless', input: {} });
    expect((await browserDialogIo({ hash: '#/dialog?id=a&dialogInput=nope' }).take()).input).toEqual({});
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    const close = vi.spyOn(window, 'close').mockImplementation(() => undefined);
    await expect(browserDialogIo({ hash: '' }).close(1)).resolves.toBeUndefined();
    set.mockRestore();
    close.mockRestore();
  });
});
