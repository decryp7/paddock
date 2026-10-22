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
    for (const id of ['general', 'server', 'memory', 'network', 'privacy', 'service', 'updates']) expect(el.querySelector('#set-' + id)).not.toBeNull();
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
});
