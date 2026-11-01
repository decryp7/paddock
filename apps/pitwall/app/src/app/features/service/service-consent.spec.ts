import { TestBed } from '@angular/core/testing';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { dialogsAnswering } from '../../../testing/render';
import { CONSENT_DIALOG_ID, ServiceConsentDialog, ServiceConsentPrompt, consentDetail, type ConsentAnswer } from './service-consent';
import { ServiceStore } from './service.store';

describe('consentDetail', () => {
  it('names who approves it on each OS', () => {
    expect(consentDetail('windows')).toContain('administrator');
    expect(consentDetail('macos')).toContain('Login Items');
    expect(consentDetail('linux')).toContain('systemd');
  });
});

describe('ServiceConsentDialog (pw.service-consent)', () => {
  const open = (platform = 'macos') => renderFramed(ServiceConsentDialog, { id: CONSENT_DIALOG_ID, kind: 'decision', modality: 'modal', inputs: { platform } });
  afterEach(() => delete document.documentElement.dataset['platform']);

  it('asks with the platform’s wording, Not now as the way out and Run in the background as the answer', async () => {
    document.documentElement.dataset['platform'] = 'macos';
    const d = await open();
    expect(d.heading()).toBe("Run Pitwall's server in the background?");
    expect(d.q('consent-detail')!.textContent).toContain('Login Items');
    expect(d.actions()).toEqual(['not-now', 'install']);
    expect(document.activeElement).toBe(d.action('install'));
    await d.press('install');
    expect(d.outcome()).toEqual({ kind: 'answered', value: { install: true, dontAsk: false } });
  });

  it('Not now and Escape answer nothing, so the question comes back next time', async () => {
    const d = await open('windows');
    expect(d.q('consent-detail')!.textContent).toContain('administrator');
    await d.press('not-now');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
    TestBed.resetTestingModule();
    const e = await open();
    await e.key('Escape');
    expect(e.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('with "Don\'t ask again" ticked, Not now (or Escape) answers never ask again', async () => {
    const d = await open();
    d.q('consent-never')!.querySelector<HTMLInputElement>('input')!.click();
    await d.stable();
    await d.press('not-now');
    expect(d.outcome()).toEqual({ kind: 'answered', value: { install: false, dontAsk: true } });
    TestBed.resetTestingModule();
    const e = await open();
    e.q('consent-never')!.querySelector<HTMLInputElement>('input')!.click();
    await e.stable();
    await e.key('Escape');
    expect(e.outcome()).toEqual({ kind: 'answered', value: { install: false, dontAsk: true } });
  });
});

describe('ServiceConsentPrompt', () => {
  async function setup(answer: ConsentAnswer | undefined, options = { askConsent: true }) {
    const backend = new MockBackend(options);
    const dialogs = dialogsAnswering(() => answer);
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, dialogs] });
    const store = TestBed.inject(ServiceStore);
    await store.refresh();
    return { prompt: TestBed.inject(ServiceConsentPrompt), store, opened: dialogs.opened, backend };
  }

  it('does not ask when there is no question pending', async () => {
    const { prompt, opened } = await setup({ install: true, dontAsk: false }, { askConsent: false });
    await prompt.ask();
    expect(opened).toEqual([]);
  });

  it('opens its window with the platform and installs the service on yes', async () => {
    const { prompt, store, opened } = await setup({ install: true, dontAsk: false });
    await prompt.ask();
    expect(opened[0]).toMatchObject({ id: CONSENT_DIALOG_ID, inputs: { platform: store.service()?.platform } });
    expect(store.service()?.installed).toBe(true);
  });

  it('remembers "Don\'t ask again"', async () => {
    const { prompt, store, backend } = await setup({ install: false, dontAsk: true });
    const spy = vi.spyOn(backend, 'declineService');
    await prompt.ask();
    expect(spy).toHaveBeenCalled();
    expect(store.askConsent()).toBe(false);
  });

  it('Not now hides the question without remembering it', async () => {
    const { prompt, store, backend } = await setup(undefined);
    const spy = vi.spyOn(backend, 'declineService');
    await prompt.ask();
    expect(store.askConsent()).toBe(false);
    expect(spy).not.toHaveBeenCalled();
  });

  it('never opens two questions at once', async () => {
    const { prompt, opened } = await setup(undefined);
    await Promise.all([prompt.ask(), prompt.ask()]);
    expect(opened).toHaveLength(1);
  });
});
