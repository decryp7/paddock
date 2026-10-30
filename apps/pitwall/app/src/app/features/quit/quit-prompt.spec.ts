import { Dialog } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { QuitPrompt } from './quit-prompt';

const tick = async () => {
  for (let i = 0; i < 6; i++) await new Promise((r) => setTimeout(r, 0));
};

const choose = async (label: string) => {
  const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>('pw-quit-dialog button'));
  buttons.find((b) => b.textContent?.includes(label))!.click();
  await tick();
};

describe('QuitPrompt', () => {
  function setup(backend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    return { backend, prompt: TestBed.inject(QuitPrompt) };
  }

  afterEach(() => TestBed.inject(Dialog).closeAll());

  it('asks when the app asks and quits keeping the service serving', async () => {
    const { backend, prompt } = setup();
    const stop = prompt.watch();
    backend.askQuit();
    await tick();
    expect(document.querySelector('pw-quit-dialog')?.textContent).toContain('Quit Pitwall?');
    await choose('Quit, keep serving');
    expect(backend.quits).toEqual([{ stopServing: false, remember: false }]);
    stop();
  });

  it('stops serving and remembers the choice when asked to', async () => {
    const { backend, prompt } = setup();
    prompt.watch();
    backend.askQuit();
    await tick();
    (document.querySelector('pw-quit-dialog mat-checkbox input') as HTMLInputElement).click();
    await choose('Quit and stop serving');
    expect(backend.quits).toEqual([{ stopServing: true, remember: true }]);
    expect((await backend.settings()).general.onQuit).toBe('stopServing');
    expect((await backend.overview()).server.serving).toBe('stopped');
  });

  it('Cancel quits nothing, and a second request while the dialog is open opens no second one', async () => {
    const { backend, prompt } = setup();
    prompt.watch();
    backend.askQuit();
    backend.askQuit();
    await tick();
    expect(document.querySelectorAll('pw-quit-dialog').length).toBe(1);
    await choose('Cancel');
    expect(backend.quits).toEqual([]);
  });

  it('shows the dialog for a quit that was asked before the window existed', async () => {
    const { backend, prompt } = setup();
    backend.askQuit(false);
    prompt.watch();
    await tick();
    expect(document.querySelector('pw-quit-dialog')).not.toBeNull();
    await choose('Cancel');
    prompt.watch();
    await tick();
    expect(document.querySelector('pw-quit-dialog'), 'asked once').toBeNull();
  });

  it('says so when quitting fails', async () => {
    const { prompt } = setup(new MockBackend({ failing: ['quitApp'] }));
    void prompt.ask();
    await tick();
    // Answering with a failing backend toasts the error and keeps the app open.
    await choose('Quit, keep serving');
    expect(TestBed.inject(Toasts).toasts().at(-1)?.tone).toBe('bad');
  });
});
