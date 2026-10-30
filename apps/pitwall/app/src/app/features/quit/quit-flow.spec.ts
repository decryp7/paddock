import { TestBed } from '@angular/core/testing';
import type { OnQuit } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { dialogsAnswering } from '../../../testing/render';
import { QUIT_DIALOG_ID, QuitFlow } from './quit-flow';

async function setup(onQuit: OnQuit, answer: unknown, backend = new MockBackend()) {
  const settings = await backend.settings();
  await backend.saveSettings({ ...settings, general: { ...settings.general, onQuit } });
  const dialogs = dialogsAnswering(() => answer);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, dialogs] });
  const quit = vi.spyOn(backend, 'quitApp');
  return { flow: TestBed.inject(QuitFlow), quit, opened: dialogs.opened, backend };
}

describe('QuitFlow', () => {
  it('"Ask each time" opens pw.quit and quits keeping serving when that is the answer', async () => {
    const { flow, quit, opened } = await setup('ask', { stopServing: false, remember: false });
    await flow.quit();
    expect(opened.map((o) => o.id)).toEqual([QUIT_DIALOG_ID]);
    expect(quit).toHaveBeenCalledWith(false, false);
  });

  it('"Ask each time" with stop serving and "Don\'t ask again" stops and remembers', async () => {
    const { flow, quit } = await setup('ask', { stopServing: true, remember: true });
    await flow.quit();
    expect(quit).toHaveBeenCalledWith(true, true);
  });

  it('a cancelled question does nothing at all', async () => {
    const { flow, quit, opened } = await setup('ask', undefined);
    await flow.quit();
    expect(opened).toHaveLength(1);
    expect(quit).not.toHaveBeenCalled();
  });

  it('"Keep serving" and "Stop serving" quit at once with the setting’s choice, without asking', async () => {
    const keep = await setup('keepServing', undefined);
    await keep.flow.quit();
    expect(keep.opened).toEqual([]);
    expect(keep.quit).toHaveBeenCalledWith(false, false);
    TestBed.resetTestingModule();
    const stop = await setup('stopServing', undefined);
    await stop.flow.quit();
    expect(stop.opened).toEqual([]);
    expect(stop.quit).toHaveBeenCalledWith(true, false);
  });

  it('"Quit Pitwall and stop serving" is forced: it never asks, whatever the setting', async () => {
    const { flow, quit, opened } = await setup('ask', undefined);
    await flow.quit('stopServing');
    expect(opened).toEqual([]);
    expect(quit).toHaveBeenCalledWith(true, false);
  });

  it('an unreadable setting quits keeping serving, so Quit always works', async () => {
    const backend = new MockBackend();
    const { flow, quit } = await setup('ask', undefined, backend);
    vi.spyOn(backend, 'settings').mockRejectedValue({ code: 'serverUnavailable', message: 'down' });
    await flow.quit();
    expect(quit).toHaveBeenCalledWith(false, false);
  });

  it('asks once at a time, and says so when quitting fails', async () => {
    const { flow, opened, backend } = await setup('ask', { stopServing: false, remember: false });
    vi.spyOn(backend, 'quitApp').mockRejectedValue({ code: 'internal', message: 'The window layer refused.' });
    await Promise.all([flow.ask(), flow.ask()]);
    expect(opened).toHaveLength(1);
    expect(TestBed.inject(Toasts).toasts().at(-1)).toMatchObject({ tone: 'bad', message: 'The window layer refused.' });
  });

  it('shows the question the app asked for, now and when it was asked before the window loaded', async () => {
    const backend = new MockBackend({ quitAsk: true });
    const { flow, opened } = await setup('ask', undefined, backend);
    const stop = flow.watch();
    await new Promise((r) => setTimeout(r, 0));
    expect(opened).toHaveLength(1);
    stop();
  });
});
