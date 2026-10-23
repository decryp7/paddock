import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { Toasts } from '../../core/toasts';
import { button, render, settle } from '../../../testing/render';
import { SettingsPage, parseNumber } from './settings-page';

describe('parseNumber', () => {
  it('clamps, and rejects empty or non-numeric input', () => {
    expect(parseNumber('5', 1, 10)).toBe(5);
    expect(parseNumber('50', 1, 10)).toBe(10);
    expect(parseNumber('-3', 1, 10)).toBe(1);
    expect(parseNumber('  ')).toBeNull();
    expect(parseNumber('abc')).toBeNull();
    expect(parseNumber('Infinity')).toBeNull();
  });
});

describe('SettingsPage', () => {
  async function setup(backend = new MockBackend()) {
    const r = await render(SettingsPage, backend);
    return { ...r, theme: TestBed.inject(ThemeService), toasts: TestBed.inject(Toasts) };
  }
  const input = (el: HTMLElement, id: string) => el.querySelector<HTMLInputElement>('#' + id)!;
  async function change(fixture: Parameters<typeof settle>[0], el: HTMLElement, id: string, value: string) {
    const i = input(el, id);
    i.value = value;
    i.dispatchEvent(new Event('change'));
    await settle(fixture);
  }
  const switchOf = (el: HTMLElement, labelId: string) => el.querySelector<HTMLButtonElement>(`button[aria-labelledby="${labelId}"], [aria-labelledby="${labelId}"] button`)!;

  beforeEach(() => localStorage.clear());

  it('renders every section from the loaded settings, with no save bar', async () => {
    const { el } = await setup();
    expect(el.querySelector('h1')?.textContent).toBe('Settings');
    for (const id of ['general', 'server', 'memory', 'network', 'model-updates', 'privacy', 'service', 'updates']) expect(el.querySelector('#set-' + id)).not.toBeNull();
    expect(input(el, 'srv-port').value).toBe('11500');
    expect(input(el, 'srv-max').value).toBe('32');
    expect(input(el, 'mem-idle').value).toBe('10');
    expect(input(el, 'mem-min').value).toBe('4');
    expect(el.querySelector('.savebar')).toBeNull();
    expect(el.textContent).toContain('Updates are not configured');
    expect(el.textContent).toContain('Not configured');
  });

  it('shows an error panel with retry when settings cannot load', async () => {
    const { el } = await setup(new MockBackend({ failing: ['settings'] }));
    expect(el.querySelector('#set-general')).toBeNull();
    expect(el.textContent).toContain('not responding');
    expect(el.querySelector('[role="alert"], .error, button')).not.toBeNull();
  });

  it('says it could not ask for updates when that check fails', async () => {
    const { el } = await setup(new MockBackend({ failing: ['appUpdateStatus'] }));
    expect(el.textContent).toContain('Could not ask for updates.');
  });

  it('shows the service state and installs and removes it', async () => {
    const { fixture, el, toasts } = await setup(new MockBackend({ askConsent: true }));
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Not installed');
    await button(el, 'Install service').click();
    await settle(fixture);
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Running');
    expect(toasts.toasts().at(-1)?.message).toBe('Pitwall now runs as a service.');
    await button(el, 'Remove service').click();
    await settle(fixture);
    expect(el.querySelector('[data-testid="service-pill"]')?.textContent).toContain('Not installed');
  });

  it('switches start at log-in', async () => {
    const { fixture, el } = await setup();
    const toggle = el.querySelector<HTMLButtonElement>('[aria-labelledby="lbl-login"] button, button[aria-labelledby="lbl-login"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    toggle.click();
    await settle(fixture);
    expect(el.querySelector<HTMLButtonElement>('button[aria-labelledby="lbl-login"]')!.getAttribute('aria-checked')).toBe('true');
  });

  it('checks for an update only on request, then installs it', async () => {
    const { fixture, el } = await setup(new MockBackend({ updateAvailable: true }));
    expect(el.textContent).toContain('only looks for updates when you ask');
    await button(el, 'Check for updates').click();
    await settle(fixture);
    expect(el.textContent).toContain('Version 0.2.0 is ready to install');
    await button(el, 'Install and restart').click();
    await settle(fixture);
    expect(el.textContent).toContain('Update available');
  });

  it('edits a number, shows the save bar, and saves it', async () => {
    const { fixture, el, toasts } = await setup();
    await change(fixture, el, 'srv-port', '12345');
    const bar = el.querySelector('.savebar')!;
    expect(bar.textContent).toContain('unsaved changes');
    button(el, 'Save changes').click();
    await settle(fixture);
    expect(el.querySelector('.savebar')).toBeNull();
    expect(toasts.toasts().at(-1)?.message).toBe('Settings saved.');
    expect(input(el, 'srv-port').value).toBe('12345');
  });

  it('shows the save error next to the bar and lets the user discard', async () => {
    const { fixture, el } = await setup();
    await change(fixture, el, 'srv-port', '80');
    expect(input(el, 'srv-port').value).toBe('80');
    button(el, 'Save changes').click();
    await settle(fixture);
    expect(el.querySelector('.savebar [role="alert"]')?.textContent).toContain('between 1024 and 65535');
    button(el, 'Discard').click();
    await settle(fixture);
    expect(el.querySelector('.savebar')).toBeNull();
    expect(input(el, 'srv-port').value).toBe('11500');
  });

  it('ignores empty or invalid numbers and clamps the rest', async () => {
    const { fixture, el, backend } = await setup();
    await change(fixture, el, 'srv-port', '');
    expect(el.querySelector('.savebar')).toBeNull();
    await change(fixture, el, 'srv-conc', '500');
    button(el, 'Save changes').click();
    await settle(fixture);
    expect((await backend.settings()).server.maxConcurrent).toBe(64);
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
    button(el, 'Save changes').click();
    await settle(fixture);
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
    button(el, 'Limit').click();
    await settle(fixture);
    const cap = el.querySelector<HTMLInputElement>('[aria-label="Memory limit for hot models in GB"]')!;
    expect(cap.value).toBe('16');
    cap.value = '24';
    cap.dispatchEvent(new Event('change'));
    await settle(fixture);
    button(el, 'Save changes').click();
    await settle(fixture);
    expect((await backend.settings()).memory.hotCapBytes).toBe(24 * 1024 ** 3);
    button(el, 'What fits').click();
    await settle(fixture);
    expect(el.querySelector('[aria-label="Memory limit for hot models in GB"]')).toBeNull();
  });

  it('applies a memory preset straight away', async () => {
    const { fixture, el, backend } = await setup();
    button(el, 'Performance').click();
    await settle(fixture);
    expect((await backend.settings()).memory.preset).toBe('performance');
    expect(input(el, 'mem-hot').value).toBe('4');
    expect(el.querySelector('.savebar')).toBeNull();
  });

  it('changes the theme live and restores the saved one on discard', async () => {
    const { fixture, el, theme } = await setup();
    button(el, 'Dark').click();
    await settle(fixture);
    expect(theme.choice()).toBe('dark');
    expect(el.querySelector('.savebar')).not.toBeNull();
    button(el, 'Discard').click();
    await settle(fixture);
    expect(theme.choice()).toBe('system');
  });

  it('toggles offline mode and saves it to the server', async () => {
    const { fixture, el, backend } = await setup();
    switchOf(el, 'lbl-offline').click();
    await settle(fixture);
    expect(el.querySelector('.savebar')).not.toBeNull();
    button(el, 'Save changes').click();
    await settle(fixture);
    expect((await backend.settings()).network.offline).toBe(true);
  });

  it('warns when keyless local access is on and shows LAN details once saved', async () => {
    const { fixture, el } = await setup();
    expect(el.querySelector('.note.warn')).toBeNull();
    switchOf(el, 'lbl-keyless').click();
    switchOf(el, 'lbl-login').click();
    switchOf(el, 'lbl-keep').click();
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
    expect(el.textContent).toContain('appear here once you save');
    await change(fixture, el, 'net-port', '12000');
    button(el, 'Save changes').click();
    await settle(fixture);
    expect(el.querySelector('dl.lan')?.textContent).toContain('https://192.168.1.42:12000');
    expect(el.querySelector('dl.lan code.fp')?.textContent).toContain('4F:9A');
  });

  it('copies the LAN address and reports a clipboard failure', async () => {
    const { fixture, el, toasts } = await setup();
    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    switchOf(el, 'lbl-lan').click();
    await settle(fixture);
    button(el, 'Save changes').click();
    await settle(fixture);
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

  it('marks the section in the sub-navigation and scrolls to it', async () => {
    const { fixture, el } = await setup();
    const target = el.querySelector<HTMLElement>('#set-privacy')!;
    const scroll = vi.fn();
    target.scrollIntoView = scroll;
    button(el, 'Privacy').click();
    await settle(fixture);
    expect(scroll).toHaveBeenCalled();
    expect(button(el, 'Privacy').getAttribute('aria-current')).toBe('true');
    expect(button(el, 'General').getAttribute('aria-current')).toBeNull();
  });

  describe('model updates', () => {
    const panel = (el: HTMLElement) => el.querySelector('#set-model-updates')!;

    it('is off by default and says plainly what is sent to Hugging Face', async () => {
      const { el } = await setup();
      const p = panel(el);
      expect(switchOf(el, 'lbl-model-updates').getAttribute('aria-checked')).toBe('false');
      expect(p.querySelector<HTMLButtonElement>('#model-updates-schedule')!.disabled).toBe(true);
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
      const trigger = panel(el).querySelector<HTMLButtonElement>('#model-updates-schedule')!;
      expect(trigger.disabled).toBe(false);
      trigger.click();
      await settle(fixture);
      Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container .pw-select-list .opt'))
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

    async function confirmWith(fixture: Parameters<typeof settle>[0], label: string) {
      await settle(fixture);
      const dlg = overlay().querySelector('pw-confirm-dialog') as HTMLElement;
      expect(dlg).not.toBeNull();
      button(dlg, label).click();
      await settle(fixture);
    }

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
      const trigger = el.querySelector<HTMLButtonElement>('#set-cache pw-select button[aria-label="Cached answers expiry: 7 days after it is stored"]')!;
      trigger.click();
      await settle(fixture);
      const options = Array.from(overlay().querySelectorAll<HTMLElement>('.pw-select-list .opt')).map((o) => o.textContent!.trim());
      expect(options).toEqual(['1 hour after it is stored', '1 day after it is stored', '7 days after it is stored', '30 days after it is stored', '90 days after it is stored', 'Never (size cap only)']);
      overlay().querySelectorAll<HTMLElement>('.pw-select-list .opt')[0]!.click();
      await settle(fixture);
      button(el, 'Save changes').click();
      await settle(fixture);
      expect((await backend.settings()).cache.responsesTtlSecs).toBe(3600);

      el.querySelector<HTMLButtonElement>('#set-cache pw-select button[aria-label^="Embeddings expiry"]')!.click();
      await settle(fixture);
      overlay().querySelectorAll<HTMLElement>('.pw-select-list .opt')[5]!.click();
      await settle(fixture);
      button(el, 'Save changes').click();
      await settle(fixture);
      expect((await backend.settings()).cache.embeddingsTtlSecs).toBe(0);
    });

    it('edits storage, size, embeddings, prompt cache and keep-warm days, saved with the rest', async () => {
      const { fixture, el, backend } = await setup();
      const radio = (group: string, text: string) => Array.from(el.querySelectorAll<HTMLButtonElement>(`[aria-labelledby="${group}"] button`)).find((b) => b.textContent!.trim() === text)!;
      radio('lbl-responses-storage', 'Memory only').click();
      radio('lbl-embeddings-storage', 'Memory only').click();
      radio('lbl-prompt-cache', 'Set size').click();
      await settle(fixture);
      await change(fixture, el, 'cache-responses-max', '128');
      await change(fixture, el, 'cache-embeddings-max', '64');
      await change(fixture, el, 'cache-keep-warm', '14');
      el.querySelector<HTMLInputElement>('input[aria-label="Prompt cache size in MiB"]')!.value = '2048';
      el.querySelector<HTMLInputElement>('input[aria-label="Prompt cache size in MiB"]')!.dispatchEvent(new Event('change'));
      switchOf(el, 'lbl-affinity').click();
      switchOf(el, 'lbl-emb-on').click();
      await settle(fixture);
      button(el, 'Save changes').click();
      await settle(fixture);
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
      radio('lbl-prompt-cache', 'Automatic').click();
      await settle(fixture);
      expect(el.querySelector('input[aria-label="Prompt cache size in MiB"]')).toBeNull();
    });

    it('clears one cache after confirming and refreshes its figures', async () => {
      const { fixture, el, toasts } = await setup();
      const clear = Array.from(el.querySelectorAll<HTMLButtonElement>('#set-cache button')).filter((b) => b.textContent!.trim() === 'Clear now');
      expect(clear.length).toBe(2);
      clear[1]!.click();
      await confirmWith(fixture, 'Clear now');
      expect(toasts.toasts().at(-1)?.message).toContain('Removed 184 cached entries');
      expect(el.querySelectorAll('#set-cache .cache-stats')[1]!.textContent).toContain('none');
    });

    it('does nothing when the confirmation is cancelled, and clears everything on request', async () => {
      const { fixture, el, toasts } = await setup();
      button(el, 'Clear all caches').click();
      await settle(fixture);
      button(overlay(), 'Cancel').click();
      await settle(fixture);
      expect(toasts.toasts().length).toBe(0);
      button(el.querySelector('#set-cache') as HTMLElement, 'Clear all caches').click();
      await confirmWith(fixture, 'Clear now');
      expect(toasts.toasts().at(-1)?.message).toContain('Removed 3596 cached entries');
    });
  });
});
