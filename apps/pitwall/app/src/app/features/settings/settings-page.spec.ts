import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { LiveStore } from '../../core/live.store';
import { Toasts } from '../../core/toasts';
import { button, dialogsAnswering, render, settle } from '../../../testing/render';
import { OWN_TOAST_HOST, SettingsPage, checkNumber } from './settings-page';
import { SETTINGS_SAVE_DELAY, SettingsStore } from './settings.store';

describe('checkNumber', () => {
  it('takes a number in range and says what to fix otherwise', () => {
    expect(checkNumber('5', 1, 10)).toEqual({ ok: true, value: 5 });
    expect(checkNumber('2.5', 0, 10)).toEqual({ ok: true, value: 2.5 });
    for (const bad of ['50', '-3', '  ', 'abc', 'Infinity']) expect(checkNumber(bad, 1, 10)).toEqual({ ok: false, error: 'Enter a number from 1 to 10.' });
  });
});

describe('SettingsPage', () => {
  /** How the confirmation's window answers (`true`: the affirmative). */
  let confirmAnswer = true;
  async function setup(backend = new MockBackend(), saveDelay = 0) {
    const dialogs = dialogsAnswering(() => (confirmAnswer ? true : undefined));
    TestBed.configureTestingModule({ providers: [dialogs, { provide: SETTINGS_SAVE_DELAY, useValue: saveDelay }] });
    const r = await render(SettingsPage, backend);
    return { ...r, toasts: TestBed.inject(Toasts), opened: dialogs.opened };
  }
  const input = (el: HTMLElement, id: string) => el.querySelector<HTMLInputElement>('#' + id)!;
  async function change(fixture: Parameters<typeof settle>[0], el: HTMLElement, id: string, value: string) {
    const i = input(el, id);
    i.value = value;
    i.dispatchEvent(new Event('change'));
    await settle(fixture);
  }
  /** Waits for the page's own save of what was edited (it saves by itself a moment after each edit). */
  async function saved(fixture: Parameters<typeof settle>[0]) {
    await settle(fixture);
    await fixture!.debugElement.injector.get(SettingsStore).flush();
    await settle(fixture);
  }
  /** Chooses `label` in the pd-select whose trigger has id `id`. */
  async function pick(fixture: Parameters<typeof settle>[0], el: HTMLElement, id: string, label: string) {
    el.querySelector<HTMLElement>('#' + id)!.click();
    await settle(fixture);
    const panels = Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container .mat-mdc-select-panel'));
    Array.from(panels.at(-1)!.querySelectorAll<HTMLElement>('mat-option'))
      .find((o) => o.textContent!.trim() === label)!
      .click();
    await settle(fixture);
  }
  const switchOf = (el: HTMLElement, labelId: string) => el.querySelector<HTMLButtonElement>(`button[aria-labelledby="${labelId}"], [aria-labelledby="${labelId}"] button`)!;

  beforeEach(() => {
    localStorage.clear();
    confirmAnswer = true;
  });

  it('renders every section from the loaded settings as the Settings window’s Pitwall page, with no save bar or save wording', async () => {
    const { el } = await setup();
    expect(el.querySelector('h2.pd-title')?.textContent).toBe('Pitwall');
    expect(el.textContent).not.toMatch(/when you save|unsaved|Save changes/i);
    expect(el.querySelector('h1')).toBeNull();
    for (const id of ['general', 'server', 'memory', 'network', 'model-updates', 'privacy', 'service']) expect(el.querySelector('#set-' + id)).not.toBeNull();
    // Appearance and app updates are the shell's pages of the window: no theme switch and no Updates section here.
    expect(el.querySelector('#set-updates, [aria-labelledby="lbl-theme"]')).toBeNull();
    expect(input(el, 'srv-port').value).toBe('11500');
    expect(input(el, 'srv-max').value).toBe('32');
    expect(input(el, 'mem-idle').value).toBe('10');
    expect(input(el, 'mem-min').value).toBe('4');
    expect(el.querySelector('.savebar')).toBeNull();
    // Rule 47: every number field is a Material field, no native select or legacy button is left.
    for (const id of ['srv-port', 'srv-max', 'mem-idle', 'prv-ret']) expect(input(el, id).closest('mat-form-field')).not.toBeNull();
    expect(el.querySelector('select, [class*="btn"]')).toBeNull();
    // Every immediate setting is a switch or a select: no segmented toggles or radio cards are left.
    expect(el.querySelector('mat-button-toggle-group, [role="radio"]')).toBeNull();
  });

  it('in a window of its own reads the overview itself and shows its confirmations there', async () => {
    TestBed.configureTestingModule({ providers: [{ provide: OWN_TOAST_HOST, useValue: true }] });
    const { el } = await setup();
    expect(TestBed.inject(LiveStore).server()).not.toBeNull();
    expect(el.querySelector('pd-notification-host')).not.toBeNull();
  });

  it('in the main window leaves the confirmations to the frame’s host', async () => {
    const { el } = await setup();
    expect(el.querySelector('pd-notification-host')).toBeNull();
  });

  it('shows an error panel with retry when settings cannot load', async () => {
    const { el } = await setup(new MockBackend({ failing: ['settings'] }));
    expect(el.querySelector('#set-general')).toBeNull();
    expect(el.textContent).toContain('not responding');
    expect(el.querySelector('[role="alert"], .error, button')).not.toBeNull();
  });

  it('shows the service state and installs and removes it', async () => {
    const { fixture, el, toasts } = await setup(new MockBackend({ askConsent: true }));
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Not installed');
    await button(el, 'Install service').click();
    await settle(fixture);
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Running');
    expect(toasts.toasts().at(-1)?.message).toBe('Pitwall now runs as a service.');
    // Removal says what it does, not how installing works; its button keeps its label on one line.
    const hint = el.querySelector('[data-testid="service-action-hint"]')!.textContent!;
    expect(hint).toContain('stops running in the background');
    expect(hint).toContain('only while Pitwall is open');
    expect(hint).toContain('models and settings stay');
    expect(hint).not.toContain('asks you to allow');
    expect(getComputedStyle(button(el, 'Remove service')).whiteSpace).toBe('nowrap');
    await button(el, 'Remove service').click();
    await settle(fixture);
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Not installed');
  });

  it('offers Open Login Items instead of Install when the item is switched off, and shows the repair', async () => {
    const backend = new MockBackend({ loginItemOff: true, repairing: true });
    const { fixture, el } = await setup(backend);
    expect(el.querySelector('[data-testid="service-repair-row"]')?.textContent).toContain('re-registering');
    expect(el.querySelector('[data-testid="service-login-items-row"]')).not.toBeNull();
    await button(el, 'Open Login Items').click();
    await settle(fixture);
    expect(backend.loginItemsOpened).toBe(1);
  });

  it('renders the Service section and repairs it while the server is down', async () => {
    const backend = new MockBackend({ serverDown: true, startFailed: true });
    const { fixture, el } = await setup(backend);
    // Nothing else of the settings loads, but the service card does.
    expect(el.querySelector('#set-general')).toBeNull();
    expect(el.querySelector('#set-service')).not.toBeNull();
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Could not start');
    expect(el.querySelector('[data-testid="service-hint"]')?.textContent).toContain('could not start after the update');
    // The server-not-running error on the page carries the same Repair.
    expect(el.querySelectorAll('[data-testid="repair-service"]').length).toBeGreaterThan(0);
    el.querySelector<HTMLButtonElement>('#set-service [data-testid="repair-service"]')!.click();
    await settle(fixture);
    expect(backend.repairs).toBe(1);
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Running');
  });

  it('switches start at log-in', async () => {
    const { fixture, el } = await setup();
    const toggle = el.querySelector<HTMLButtonElement>('[aria-labelledby="lbl-login"] button, button[aria-labelledby="lbl-login"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    toggle.click();
    await settle(fixture);
    expect(el.querySelector<HTMLButtonElement>('button[aria-labelledby="lbl-login"]')!.getAttribute('aria-checked')).toBe('true');
  });

  it('saves an edited number by itself, with no save bar and no toast', async () => {
    const { fixture, el, toasts, backend } = await setup();
    await change(fixture, el, 'srv-port', '12345');
    expect(el.querySelector('.savebar')).toBeNull();
    await saved(fixture);
    expect((await backend.settings()).server.port).toBe(12345);
    expect(input(el, 'srv-port').value).toBe('12345');
    expect(el.querySelector('[data-testid="save-error"]')).toBeNull();
    expect(toasts.toasts()).toEqual([]);
  });

  it('a refused save says so inline, keeps the edit, and Undo change puts the saved value back', async () => {
    const { fixture, el, backend } = await setup();
    const real = backend.saveSettings.bind(backend);
    const save = vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'The Pitwall server is not responding.' });
    await change(fixture, el, 'srv-timeout', '120');
    await saved(fixture);
    const error = el.querySelector('[data-testid="save-error"]')!;
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent).toContain('Your last change was not saved.');
    expect(error.textContent).toContain('not responding');
    expect(input(el, 'srv-timeout').value).toBe('120');
    // Retry saves the kept edit once the server answers.
    save.mockImplementation(real);
    button(error as HTMLElement, 'Retry').click();
    await saved(fixture);
    expect(el.querySelector('[data-testid="save-error"]')).toBeNull();
    expect((await backend.settings()).server.requestTimeoutSecs).toBe(120);

    save.mockRejectedValueOnce({ code: 'badRequest', message: 'No.' });
    await change(fixture, el, 'srv-timeout', '90');
    await saved(fixture);
    button(el.querySelector('[data-testid="save-error"]') as HTMLElement, 'Undo change').click();
    await settle(fixture);
    expect(el.querySelector('[data-testid="save-error"]')).toBeNull();
    expect(input(el, 'srv-timeout').value).toBe('120');
  });

  it('says inline what to fix in a number out of range or empty, and saves nothing for it', async () => {
    const { fixture, el, backend } = await setup();
    const save = vi.spyOn(backend, 'saveSettings');
    await change(fixture, el, 'srv-port', '80');
    await change(fixture, el, 'srv-conc', '');
    await saved(fixture);
    expect(save).not.toHaveBeenCalled();
    expect(input(el, 'srv-port').getAttribute('aria-invalid')).toBe('true');
    expect(input(el, 'srv-port').closest('mat-form-field')!.classList).toContain('invalid');
    expect(el.querySelector('#set-server')!.textContent).toContain('Enter a number from 1024 to 65535.');
    expect(el.querySelector('#set-server')!.textContent).toContain('Enter a number from 1 to 64.');
    await change(fixture, el, 'srv-port', '12000');
    await saved(fixture);
    expect(input(el, 'srv-port').getAttribute('aria-invalid')).toBe('false');
    expect(el.querySelector('#set-server')!.textContent).not.toContain('from 1024 to 65535');
    expect((await backend.settings()).server.port).toBe(12000);
  });

  it('loses nothing when the window closes right after an edit', async () => {
    // A long wait, so only the close can have saved it.
    const { fixture, el, backend } = await setup(new MockBackend(), 60_000);
    await change(fixture, el, 'prv-ret', '14');
    expect((await backend.settings()).privacy.metricsRetentionDays).toBe(30);
    fixture.destroy();
    await vi.waitFor(async () => expect((await backend.settings()).privacy.metricsRetentionDays).toBe(14));
  });

  it('saves an edit still waiting when the window hides', async () => {
    const { fixture, el, backend } = await setup(new MockBackend(), 60_000);
    await change(fixture, el, 'prv-ret', '21');
    window.dispatchEvent(new Event('pagehide'));
    await vi.waitFor(async () => expect((await backend.settings()).privacy.metricsRetentionDays).toBe(21));
  });

  it('converts units for every numeric field', async () => {
    const { fixture, el, backend } = await setup();
    await change(fixture, el, 'srv-max', '64');
    await change(fixture, el, 'srv-timeout', '120');
    await change(fixture, el, 'mem-idle', '5');
    await change(fixture, el, 'mem-deep', '20');
    await change(fixture, el, 'mem-pressure', '20');
    await change(fixture, el, 'mem-min', '2.5');
    await change(fixture, el, 'mem-hot', '2');
    await change(fixture, el, 'prv-ret', '14');
    await saved(fixture);
    const s = await backend.settings();
    expect(s.server.maxRequestBytes).toBe(64 * 1024 ** 2);
    expect(s.server.requestTimeoutSecs).toBe(120);
    expect(s.memory.idleUnloadSecs).toBe(300);
    expect(s.memory.deepIdleSecs).toBe(1200);
    expect(s.memory.pressureAvailablePercent).toBe(20);
    expect(s.memory.minFreeBytes).toBe(2.5 * 1024 ** 3);
    expect(s.memory.maxHotModels).toBe(2);
    expect(s.memory.preset).toBe('custom');
    expect(s.privacy.metricsRetentionDays).toBe(14);
  });

  it('switches the memory hot cap between what fits and a limit', async () => {
    const { fixture, el, backend } = await setup();
    expect(el.querySelector('[aria-label="Memory limit for hot models in GB"]')).toBeNull();
    await pick(fixture, el, 'mem-cap-mode', 'Set a limit');
    const cap = el.querySelector<HTMLInputElement>('[aria-label="Memory limit for hot models in GB"]')!;
    expect(cap.value).toBe('16');
    await change(fixture, el, 'mem-cap', '24');
    await saved(fixture);
    expect((await backend.settings()).memory.hotCapBytes).toBe(24 * 1024 ** 3);
    await pick(fixture, el, 'mem-cap-mode', 'What fits');
    expect(el.querySelector('[aria-label="Memory limit for hot models in GB"]')).toBeNull();
  });

  it('applies a memory preset straight away from its select; Custom is shown, never chosen', async () => {
    const { fixture, el, backend } = await setup();
    expect(el.querySelector('#mem-preset')!.textContent).toContain('Balanced');
    el.querySelector<HTMLElement>('#mem-preset')!.click();
    await settle(fixture);
    const custom = Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container mat-option')).find((o) => o.textContent!.trim() === 'Custom')!;
    expect(custom.getAttribute('aria-disabled')).toBe('true');
    Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container mat-option'))
      .find((o) => o.textContent!.trim() === 'Performance')!
      .click();
    await settle(fixture);
    expect((await backend.settings()).memory.preset).toBe('performance');
    expect(input(el, 'mem-hot').value).toBe('4');
    expect(el.querySelector('#set-memory')!.textContent).toContain('Keep models hot, use what fits.');
    await change(fixture, el, 'mem-hot', '2');
    expect(el.querySelector('#mem-preset')!.textContent).toContain('Custom');
    expect(el.querySelector('#set-memory')!.textContent).toContain('Your own values below.');
  });

  it('toggles offline mode and saves it to the server', async () => {
    const { fixture, el, backend } = await setup();
    switchOf(el, 'lbl-offline').click();
    await saved(fixture);
    expect((await backend.settings()).network.offline).toBe(true);
  });

  it('chooses what quitting does about serving from a select and saves it', async () => {
    const { fixture, el, backend } = await setup();
    expect(el.querySelector('#gen-quit')!.textContent).toContain('Keep serving');
    await pick(fixture, el, 'gen-quit', 'Ask each time');
    await saved(fixture);
    expect((await backend.settings()).general.onQuit).toBe('ask');
    expect(el.querySelector('#gen-quit')!.textContent).toContain('Ask each time');
  });

  it('warns when keyless local access is on and shows LAN details once saved', async () => {
    const { fixture, el } = await setup();
    expect(el.querySelector('.note.warn')).toBeNull();
    switchOf(el, 'lbl-keyless').click();
    switchOf(el, 'lbl-login').click();
    switchOf(el, 'lbl-ctx').click();
    switchOf(el, 'lbl-yield').click();
    switchOf(el, 'lbl-notify').click();
    switchOf(el, 'lbl-content').click();
    await settle(fixture);
    expect(el.querySelector('.note.warn')).not.toBeNull();

    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    switchOf(el, 'lbl-lan').click();
    await settle(fixture);
    expect(el.querySelector('.note.info')).not.toBeNull();
    await change(fixture, el, 'net-port', '12000');
    await saved(fixture);
    expect(el.querySelector('dl.lan')?.textContent).toContain('https://192.168.1.42:12000');
    expect(el.querySelector('dl.lan code.fp')?.textContent).toContain('4F:9A');
  });

  it('copies the LAN address and reports a clipboard failure', async () => {
    const { fixture, el, toasts } = await setup();
    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    switchOf(el, 'lbl-lan').click();
    await saved(fixture);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    button(el, 'Copy address').click();
    await settle(fixture);
    expect(writeText).toHaveBeenCalledWith('https://192.168.1.42:11501');
    expect(toasts.toasts().at(-1)?.message).toBe('Copied.');
    writeText.mockRejectedValue(new Error('denied'));
    button(el, 'Copy fingerprint').click();
    await settle(fixture);
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'bad' });
  });

  describe('model updates', () => {
    const panel = (el: HTMLElement) => el.querySelector('#set-model-updates')!;

    it('is off by default and says plainly what is sent to Hugging Face', async () => {
      const { el } = await setup();
      const p = panel(el);
      expect(switchOf(el, 'lbl-model-updates').getAttribute('aria-checked')).toBe('false');
      expect(p.querySelector<HTMLElement>('#model-updates-schedule')!.getAttribute('aria-disabled')).toBe('true');
      expect(p.textContent).toContain('Off until you turn it on');
      expect(p.textContent).toContain('never downloads anything by itself');
      expect(p.textContent).toContain('only the repository ids');
      expect(p.textContent).toContain('Never checked.');
      expect(p.textContent).toContain('Scheduled checks are off; Check now still works.');
    });

    it('turns the schedule on, lets you pick daily, and shows the next check', async () => {
      const { el, fixture, backend } = await setup();
      const spy = vi.spyOn(backend, 'setUpdateCheck');
      switchOf(el, 'lbl-model-updates').click();
      await settle(fixture);
      expect(spy).toHaveBeenLastCalledWith(true, 'weekly');
      const trigger = panel(el).querySelector<HTMLElement>('#model-updates-schedule')!;
      expect(trigger.getAttribute('aria-disabled')).toBe('false');
      trigger.click();
      await settle(fixture);
      Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container mat-option'))
        .find((o) => o.textContent!.trim() === 'Every day')!
        .click();
      await settle(fixture);
      expect(spy).toHaveBeenLastCalledWith(true, 'daily');
      expect(panel(el).textContent).toContain('The next check runs');
    });

    it('checks now and shows what it found', async () => {
      const { el, fixture } = await setup();
      button(panel(el) as HTMLElement, 'Check now').click();
      await settle(fixture);
      expect(panel(el).textContent).toContain('Asked Hugging Face about');
      expect(panel(el).textContent).toContain('Last checked');
      expect(TestBed.inject(Toasts).toasts().at(-1)?.message).toBe('Checked: 2 updates available.');
    });

    it('cannot check while offline mode is on, and says so', async () => {
      const mock = new MockBackend();
      const s = await mock.settings();
      await mock.saveSettings({ ...s, network: { ...s.network, offline: true } });
      const { el } = await setup(mock);
      expect(button(panel(el) as HTMLElement, 'Check now').disabled).toBe(true);
      expect(panel(el).textContent).toContain('Offline mode is on, so no check can run.');
    });

    it('shows an error with Retry when the state cannot be read', async () => {
      const { el } = await setup(new MockBackend({ failing: ['updateCheckStatus'] }));
      expect(panel(el).querySelector('#model-updates-schedule')).toBeNull();
      expect(panel(el).textContent).toContain('Retry');
    });
  });

  describe('caching', () => {
    const overlay = () => document.querySelector('.cdk-overlay-container') as HTMLElement;
    afterEach(() => overlay()?.replaceChildren());

    it('explains the three layers and the privacy rule in plain words', async () => {
      const { el } = await setup();
      const card = el.querySelector('#set-cache')!;
      expect(card.textContent).toContain('Prompt reuse');
      expect(card.textContent).toContain('opt-in for each app and each preset');
      expect(card.textContent).toContain('temperature 0 or a fixed seed');
      expect(card.textContent).toContain('cache.sqlite');
      expect(card.textContent).toContain('never sent anywhere');
      expect(card.textContent).toContain('Memory only');
    });

    it('shows the stats of each cache, the shared next clean-up and no disk error', async () => {
      const { el } = await setup();
      const stats = Array.from(el.querySelectorAll('#set-cache .cache-stats'));
      expect(stats.length).toBe(2);
      expect(stats[0]!.textContent).toContain('Entries');
      expect(stats[0]!.textContent).toContain('90 days since last use');
      expect(stats[1]!.textContent).toContain('7 days after it is stored');
      expect(stats[1]!.textContent).toMatch(/Oldest entry\s*5 d ago|Oldest entry\s*\d+ d/);
      expect(el.querySelector('[data-testid="next-sweep"]')!.textContent!.length).toBeGreaterThan(3);
      expect(el.querySelector('#set-cache [role="alert"]')).toBeNull();
    });

    it('shows a disk error inline and keeps the page working', async () => {
      const backend = new MockBackend();
      const real = backend.cacheInfo.bind(backend);
      vi.spyOn(backend, 'cacheInfo').mockImplementation(async () => {
        const i = await real();
        return { ...i, responses: { ...i.responses, diskError: 'cache.sqlite is locked by another program', oldestAt: null, entries: 0 } };
      });
      const { el } = await setup(backend);
      const alert = el.querySelector('#set-cache [role="alert"]')!;
      expect(alert.textContent).toContain('memory only for now: cache.sqlite is locked');
      expect(el.querySelectorAll('#set-cache .cache-stats')[1]!.textContent).toContain('none');
    });

    it('shows an inline error with retry when the figures cannot be read', async () => {
      const { el, fixture } = await setup(new MockBackend({ failing: ['cacheInfo'] }));
      expect(el.querySelector('#set-cache [role="alert"]')!.textContent).toContain('Could not read the cache figures');
      expect(el.querySelectorAll('#set-cache .cache-stats').length).toBe(0);
      button(el.querySelector('#set-cache') as HTMLElement, 'Retry').click();
      await settle(fixture);
    });

    it('maps the expiry choices to seconds in the draft', async () => {
      const { fixture, el, backend } = await setup();
      const trigger = el.querySelector<HTMLElement>('#set-cache pd-select mat-select[aria-label="Cached answers expiry"]')!;
      expect(trigger.textContent).toContain('7 days after it is stored');
      trigger.click();
      await settle(fixture);
      const options = Array.from(overlay().querySelectorAll<HTMLElement>('mat-option')).map((o) => o.textContent!.trim());
      expect(options).toEqual(['1 hour after it is stored', '1 day after it is stored', '7 days after it is stored', '30 days after it is stored', '90 days after it is stored', 'Never (size cap only)']);
      overlay().querySelectorAll<HTMLElement>('mat-option')[0]!.click();
      await saved(fixture);
      expect((await backend.settings()).cache.responsesTtlSecs).toBe(3600);

      el.querySelector<HTMLElement>('#set-cache pd-select mat-select[aria-label="Embeddings expiry"]')!.click();
      await settle(fixture);
      Array.from(overlay().querySelectorAll<HTMLElement>('.mat-mdc-select-panel')).at(-1)!.querySelectorAll<HTMLElement>('mat-option')[5]!.click();
      await saved(fixture);
      expect((await backend.settings()).cache.embeddingsTtlSecs).toBe(0);
    });

    it('edits storage, size, embeddings, prompt cache and keep-warm days, saved with the rest', async () => {
      const { fixture, el, backend } = await setup();
      await pick(fixture, el, 'cache-responses-storage', 'Memory only');
      await pick(fixture, el, 'cache-embeddings-storage', 'Memory only');
      await pick(fixture, el, 'cache-prompt-mode', 'Set a size');
      await change(fixture, el, 'cache-responses-max', '128');
      await change(fixture, el, 'cache-embeddings-max', '64');
      await change(fixture, el, 'cache-keep-warm', '14');
      el.querySelector<HTMLInputElement>('input[aria-label="Prompt cache size in MiB"]')!.value = '2048';
      el.querySelector<HTMLInputElement>('input[aria-label="Prompt cache size in MiB"]')!.dispatchEvent(new Event('change'));
      switchOf(el, 'lbl-affinity').click();
      switchOf(el, 'lbl-emb-on').click();
      await saved(fixture);
      expect((await backend.settings()).cache).toMatchObject({
        responsesStorage: 'memory',
        embeddingsStorage: 'memory',
        responsesMaxMib: 128,
        embeddingsMaxMib: 64,
        keepWarmDays: 14,
        promptCacheMib: 2048,
        prefixAffinity: false,
        embeddings: false,
      });
      await pick(fixture, el, 'cache-prompt-mode', 'Automatic');
      expect(el.querySelector('input[aria-label="Prompt cache size in MiB"]')).toBeNull();
    });

    it('clears one cache after confirming and refreshes its figures', async () => {
      const { fixture, el, toasts, opened } = await setup();
      const clear = Array.from(el.querySelectorAll<HTMLButtonElement>('#set-cache button')).filter((b) => b.textContent!.trim() === 'Clear now');
      expect(clear.length).toBe(2);
      clear[1]!.click();
      await settle(fixture);
      expect(opened.at(-1)).toMatchObject({ id: 'pd.confirm', inputs: { confirm: { label: 'Clear cache', destructive: true } } });
      expect(toasts.toasts().at(-1)?.message).toContain('Removed 184 cached entries');
      expect(el.querySelectorAll('#set-cache .cache-stats')[1]!.textContent).toContain('none');
    });

    it('does nothing when the confirmation is cancelled, and clears everything on request', async () => {
      const { fixture, el, toasts } = await setup();
      confirmAnswer = false;
      button(el, 'Clear all caches').click();
      await settle(fixture);
      expect(toasts.toasts().length).toBe(0);
      confirmAnswer = true;
      button(el.querySelector('#set-cache') as HTMLElement, 'Clear all caches').click();
      await settle(fixture);
      expect(toasts.toasts().at(-1)?.message).toContain('Removed 3596 cached entries');
    });
  });
});
