import { Dialog, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { of } from 'rxjs';
import { MockBackend } from '../../core/mock-backend';
import { render, settle } from '../../../testing/render';
import { ServiceConsent, ServiceConsentPrompt, consentDetail } from './service-consent';
import { ServiceStore } from './service.store';

describe('consentDetail', () => {
  it('names who approves it on each OS', () => {
    expect(consentDetail('windows')).toContain('administrator');
    expect(consentDetail('macos')).toContain('Login Items');
    expect(consentDetail('linux')).toContain('systemd');
  });
});

describe('ServiceConsent', () => {
  it('shows the question with the platform wording and three answers', async () => {
    const close = vi.fn();
    TestBed.configureTestingModule({ providers: [{ provide: DialogRef, useValue: { close } }] });
    const { el } = await render(ServiceConsent, new MockBackend({ askConsent: true }));
    expect(el.querySelector('h2')?.textContent).toContain('background');
    expect(Array.from(el.querySelectorAll('button')).map((b) => b.textContent?.trim())).toEqual(['No thanks', 'Not now', 'Run in the background']);
    for (const [i, answer] of ['decline', 'later', 'install'].entries()) {
      el.querySelectorAll('button')[i]?.click();
      expect(close).toHaveBeenLastCalledWith(answer);
    }
  });
});

describe('ServiceConsentPrompt', () => {
  async function setup(answer: string | undefined, options = { askConsent: true }) {
    const backend = new MockBackend(options);
    const open = vi.fn(() => ({ closed: of(answer) }));
    TestBed.configureTestingModule({ providers: [{ provide: DialogRef, useValue: { close: vi.fn() } }, { provide: Dialog, useValue: { open } }] });
    await render(ServiceConsent, backend);
    const store = TestBed.inject(ServiceStore);
    await store.refresh();
    return { prompt: TestBed.inject(ServiceConsentPrompt), store, open, backend };
  }

  it('does not ask when there is no question pending', async () => {
    const { prompt, open } = await setup('install', { askConsent: false });
    await prompt.ask();
    expect(open).not.toHaveBeenCalled();
  });

  it('installs the service on yes', async () => {
    const { prompt, store } = await setup('install');
    await prompt.ask();
    expect(store.service()?.installed).toBe(true);
  });

  it('remembers no', async () => {
    const { prompt, store, backend } = await setup('decline');
    const spy = vi.spyOn(backend, 'declineService');
    await prompt.ask();
    expect(spy).toHaveBeenCalled();
    expect(store.askConsent()).toBe(false);
  });

  it('later and a dismissed dialog hide the question without remembering it', async () => {
    const a = await setup('later');
    const spy = vi.spyOn(a.backend, 'declineService');
    await a.prompt.ask();
    expect(a.store.askConsent()).toBe(false);
    expect(spy).not.toHaveBeenCalled();
    TestBed.resetTestingModule();
    const b = await setup(undefined);
    await b.prompt.ask();
    expect(b.store.askConsent()).toBe(false);
  });

  it('never opens two questions at once', async () => {
    const { prompt, open } = await setup('later');
    await Promise.all([prompt.ask(), prompt.ask()]);
    await settle();
    expect(open).toHaveBeenCalledTimes(1);
  });
});
