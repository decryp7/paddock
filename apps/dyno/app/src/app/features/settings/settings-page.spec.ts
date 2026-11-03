import { type ComponentFixture, TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Notifier } from '../../shared/notifier';
import { SettingsStore } from '../../state/settings.store';
import { signal } from '@angular/core';
import { SETTINGS_ANCHOR, type SettingsAnchor } from '@decryp7/paddock-ui/shell/settings/window';
import { SettingsPage } from './settings-page';

const NOW = 1_700_000_000_000;

/** Renders, then lets the mock's promises and timers run and renders again (signals set after awaits). */
async function settle(f: ComponentFixture<unknown>) {
  f.detectChanges();
  // Until the store has its answer (the settings, or the error the page shows) and the plugins list was asked for.
  const store = TestBed.inject(SettingsStore);
  await vi.waitFor(() => {
    expect(store.settings() !== null || store.error() !== null).toBe(true);
    expect(store.pluginsLoaded() || store.pluginsError() !== null).toBe(true);
  });
  f.detectChanges();
  await f.whenStable();
  f.detectChanges();
}

async function make() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(SettingsPage);
  await settle(f);
  const el = f.nativeElement as HTMLElement;
  const saveButton = () => [...el.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes('Save changes'));
  const discardButton = () => [...el.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes('Discard'));
  return { f, mock, el, store: TestBed.inject(SettingsStore), saveButton, discardButton };
}
const toggle = (el: HTMLElement, label: string) => el.querySelector<HTMLButtonElement>(`button[role="switch"][aria-label="${label}"]`)!;

describe('SettingsPage', () => {
  it('renders the sections once loaded and shows no save bar while clean', async () => {
    const { el, saveButton, store } = await make();
    expect(el.querySelector('h2.pd-title')?.textContent).toBe('Dyno');
    // Appearance, Updates and About are the shell's pages of the Settings window, not Dyno's.
    for (const name of ['Start-up', 'Measuring', 'Menu bar', 'Alerts', 'Plugins']) expect(el.querySelector(`section[aria-label="${name}"]`)).not.toBeNull();
    expect(saveButton()).toBeUndefined();
    expect(store.settings()).not.toBeNull();
  });
  it('shows the plugins from the engine and no appearance or About of its own', async () => {
    const { el } = await make();
    expect(el.querySelectorAll('.plugins li').length).toBeGreaterThan(0);
    expect(el.querySelector('section[aria-label="Appearance"]')).toBeNull();
    expect(el.querySelector('section[aria-label="About"]')).toBeNull();
  });
  it('a change brings up the save bar; Save sends the whole draft and the bar goes away', async () => {
    const { f, mock, el, saveButton, store } = await make();
    const spy = vi.spyOn(mock, 'saveSettings');
    const before = store.settings()!;
    toggle(el, 'Start Dyno at login').click();
    f.detectChanges();
    expect(saveButton()).toBeDefined();
    saveButton()!.click();
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]![0]).toEqual({ ...before, launchAtLogin: !before.launchAtLogin });
    expect(store.settings()!.launchAtLogin).toBe(!before.launchAtLogin);
    expect(saveButton()).toBeUndefined();
    expect(TestBed.inject(Notifier).recent().at(-1)?.text).toBe('Settings saved');
  });
  it('Discard restores the saved values', async () => {
    const { f, mock, el, saveButton, discardButton, store } = await make();
    const spy = vi.spyOn(mock, 'saveSettings');
    toggle(el, 'Alerts').click();
    f.detectChanges();
    expect(saveButton()).toBeDefined();
    discardButton()!.click();
    f.detectChanges();
    expect(saveButton()).toBeUndefined();
    expect(spy).not.toHaveBeenCalled();
    expect(toggle(el, 'Alerts').getAttribute('aria-checked')).toBe(String(store.settings()!.alerts.enabled));
  });
  it('has the menu bar items and the popup widgets as choosers over the engine layouts, and Save never carries them', async () => {
    const { f, el, mock } = await make();
    const card = el.querySelector('section[aria-label="Menu bar"]')!;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(card.querySelectorAll('pd-widget-chooser')).toHaveLength(2);
    });
    expect(card.querySelector('#popup-widgets')).not.toBeNull();
    expect(card.querySelectorAll('dy-menu-bar-items .pd-wc-list')[0]!.querySelectorAll('li')).toHaveLength(4);
    expect(Object.keys(await mock.settings())).not.toContain('trayItems');
  });
  it('scrolls to the section the Settings window was asked for, and again when it is asked again', async () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true);
    Element.prototype.scrollIntoView = () => undefined;
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    const anchor = signal<SettingsAnchor | null>({ id: 'popup-widgets', at: 1 });
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }, { provide: SETTINGS_ANCHOR, useValue: anchor }] });
    const f = TestBed.createComponent(SettingsPage);
    await settle(f);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(scroll).toHaveBeenCalled();
    });
    expect(scroll.mock.contexts.at(-1)).toBe((f.nativeElement as HTMLElement).querySelector('#popup-widgets'));
    const times = scroll.mock.calls.length;
    anchor.set({ id: 'popup-widgets', at: 2 });
    await vi.waitFor(() => {
      f.detectChanges();
      expect(scroll.mock.calls.length).toBeGreaterThan(times);
    });
  });
  it('a rejected save keeps the draft and the bar and toasts the engine message', async () => {
    const { f, mock, el, saveButton } = await make();
    vi.spyOn(mock, 'saveSettings').mockRejectedValue({ code: 'badRequest', message: 'Nope.' });
    toggle(el, 'Start Dyno at login').click();
    f.detectChanges();
    saveButton()!.click();
    await settle(f);
    expect(saveButton()).toBeDefined();
    expect(TestBed.inject(Notifier).recent().at(-1)).toMatchObject({ tone: 'bad', text: 'Nope.' });
  });
  it('a save that never reached the engine (agent not running) puts the control back to the stored value and offers Retry', async () => {
    // Regression (Dyno login blockers): the page kept the unsaved switch position after "The Dyno agent is not running", so the
    // next press equalled the stored value and nothing was ever saved.
    const { f, mock, el, saveButton, store } = await make();
    const stored = store.settings()!.launchAtLogin;
    const spy = vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'agentUnavailable', message: 'The Dyno agent is not running.' });
    toggle(el, 'Start Dyno at login').click();
    f.detectChanges();
    saveButton()!.click();
    await settle(f);
    expect(toggle(el, 'Start Dyno at login').getAttribute('aria-checked')).toBe(String(stored));
    expect(saveButton()).toBeUndefined();
    expect(el.querySelector('.save-error')!.textContent).toContain('The Dyno agent is not running.');
    const retry = [...el.querySelectorAll<HTMLButtonElement>('header button')].find((b) => b.textContent?.includes('Retry'));
    expect(retry).toBeDefined();
    retry!.click();
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(spy.mock.calls[1]![0].launchAtLogin).toBe(!stored);
    expect(store.settings()!.launchAtLogin).toBe(!stored);
    expect(el.querySelector('.save-error')).toBeNull();
    expect([...el.querySelectorAll('header button')].some((b) => b.textContent?.includes('Retry'))).toBe(false);
  });
  it('a refusal by the engine keeps the edit so it can be corrected, and a new edit drops a pending Retry', async () => {
    const { f, mock, el, saveButton } = await make();
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'agentUnavailable', message: 'The Dyno agent is not running.' });
    toggle(el, 'Start Dyno at login').click();
    f.detectChanges();
    saveButton()!.click();
    await settle(f);
    toggle(el, 'Alerts').click();
    f.detectChanges();
    expect([...el.querySelectorAll('header button')].some((b) => b.textContent?.includes('Retry'))).toBe(false);
  });
  it('shows an inline error with Retry when settings cannot load', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(['settings']) });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(SettingsPage);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.textContent).toContain('The mock was asked to fail');
    expect([...el.querySelectorAll('button')].some((b) => /retry/i.test(b.textContent ?? ''))).toBe(true);
    expect(el.querySelector('section[aria-label="Appearance"]')).toBeNull();
  });

  it('has a Backup health section: switch, hours and folders for the tools found by name', async () => {
    const { f, el, saveButton, store } = await make();
    const section = el.querySelector('section[aria-label="Backup health"]')!;
    expect(section).not.toBeNull();
    expect(section.textContent).toContain('never runs or manages a backup');
    expect(section.querySelectorAll('textarea')).toHaveLength(3);
    expect(section.textContent).toContain('Where restic is installed');
    toggle(el, 'Tell me when a backup is out of date').click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(store.settings()!.backup.alertWhenStale).toBe(true);
      expect(saveButton()).toBeDefined();
    });
  });

  it('keeps what is typed in a folder box, validates it like the engine and saves it', async () => {
    const { f, el, saveButton, store } = await make();
    const box = el.querySelector<HTMLTextAreaElement>('textarea[aria-label="restic folders"]')!;
    box.value = 'tools/restic';
    box.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el.querySelector('section[aria-label="Backup health"] .problem')!.textContent).toContain('not a full path');
    expect(saveButton()!.disabled).toBe(true);
    box.value = '/opt/restic\n';
    box.dispatchEvent(new Event('input'));
    await settle(f);
    expect(box.value).toBe('/opt/restic\n');
    expect(el.querySelector('section[aria-label="Backup health"] .problem')).toBeNull();
    const hours = el.querySelector<HTMLInputElement>('input[aria-label="Hours until a backup is out of date"]')!;
    hours.value = '24';
    hours.dispatchEvent(new Event('input'));
    await settle(f);
    saveButton()!.click();
    await settle(f);
    expect(store.settings()!.backup).toEqual({ alertWhenStale: true, staleAfterHours: 24, overrides: [{ tool: 'restic', dirs: ['/opt/restic'] }] });
    expect(saveButton()).toBeUndefined();
    expect(el.querySelector<HTMLTextAreaElement>('textarea[aria-label="restic folders"]')!.value).toBe('/opt/restic');
  });

  it('shows the engine sentence when it refuses the backup settings, and discard restores the boxes', async () => {
    const { f, el, mock, saveButton, discardButton } = await make();
    const hours = el.querySelector<HTMLInputElement>('input[aria-label="Hours until a backup is out of date"]')!;
    hours.value = '0';
    hours.dispatchEvent(new Event('input'));
    await settle(f);
    expect(saveButton()!.disabled).toBe(true);
    hours.value = '30';
    hours.dispatchEvent(new Event('input'));
    await settle(f);
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'restic: not allowed.' });
    saveButton()!.click();
    await settle(f);
    expect(el.querySelector('.save-error')!.textContent).toBe('restic: not allowed.');
    const box = el.querySelector<HTMLTextAreaElement>('textarea[aria-label="Duplicati folders"]')!;
    box.value = '/x';
    box.dispatchEvent(new Event('input'));
    await settle(f);
    discardButton()!.click();
    await settle(f);
    expect(el.querySelector<HTMLTextAreaElement>('textarea[aria-label="Duplicati folders"]')!.value).toBe('');
  });
  it('shows the hardware and baselines section with the defaults when the settings hold none', async () => {
    const { el } = await make();
    const section = el.querySelector('section[aria-label="Hardware and baselines"]')!;
    expect(section).not.toBeNull();
    expect(toggle(el, 'Learn what is normal').getAttribute('aria-checked')).toBe('true');
    expect(toggle(el, 'Alarm when far above usual').getAttribute('aria-checked')).toBe('false');
    expect(toggle(el, 'Alarm for throttling').getAttribute('aria-checked')).toBe('true');
    expect(toggle(el, 'Alarm for disks and batteries near end of life').getAttribute('aria-checked')).toBe('true');
    expect(section.querySelector<HTMLInputElement>('input[aria-label="Days of learning"]')!.value).toBe('7');
  });
  it('changes and saves the baselines and the hardware alarms', async () => {
    const { f, mock, el, saveButton } = await make();
    toggle(el, 'Alarm when far above usual').click();
    toggle(el, 'Alarm for throttling').click();
    toggle(el, 'Alarm for disks and batteries near end of life').click();
    f.detectChanges();
    const input = el.querySelector<HTMLInputElement>('input[aria-label="Days of learning"]')!;
    input.value = '14';
    input.dispatchEvent(new Event('input'));
    f.detectChanges();
    const spy = vi.spyOn(mock, 'saveSettings');
    saveButton()!.click();
    await settle(f);
    const sent = spy.mock.calls[0]![0];
    expect(sent.baselines).toEqual({ enabled: true, learningDays: 14, alarms: true });
    expect(sent.hardwareAlarms).toEqual({ throttling: false, lifespan: false });
  });
  it('shows the disk map limits with the engine defaults, saves a change and says what is wrong with a value out of range', async () => {
    const { f, mock, el, saveButton } = await make();
    const section = el.querySelector('section[aria-label="Disk map"]')!;
    const items = section.querySelector<HTMLInputElement>('input[aria-label="Items kept per scan"]')!;
    const size = section.querySelector<HTMLInputElement>('input[aria-label="Large file size"]')!;
    expect([items.value, size.value]).toEqual(['2000000', '10']);
    const type = (i: HTMLInputElement, v: string) => {
      i.value = v;
      i.dispatchEvent(new Event('input'));
      f.detectChanges();
    };
    for (const bad of ['9999', '5000001', '1.5', '']) type(items, bad);
    for (const bad of ['0', '1025']) type(size, bad);
    expect(section.querySelector('mat-error')?.textContent).toContain('whole number from 10,000 to 5,000,000');
    expect(saveButton()!.disabled).toBe(true);
    type(items, '500000');
    type(size, '64');
    const spy = vi.spyOn(mock, 'saveSettings');
    saveButton()!.click();
    await settle(f);
    expect(spy.mock.calls[0]![0].disk).toMatchObject({ scanFolders: 500_000, notableFileMib: 64 });
  });
  it('shows Parallelism and cost with the engine defaults (Auto, 500,000 files, 48 levels, 64 GiB), says what is wrong and saves a change', async () => {
    const { f, mock, el, saveButton } = await make();
    const section = el.querySelector('section[aria-label="Parallelism and cost"]')!;
    const field = (label: string) => section.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
    expect(['File finder: files looked at', 'File finder: folder depth', 'File finder: content compared'].map((l) => field(l).value)).toEqual(['500000', '48', '64']);
    expect(section.querySelector('pd-select[ariaLabel="Threads reading folders"], [aria-label="Threads reading folders"]')).not.toBeNull();
    expect(section.querySelector('[aria-label="Jobs at once"]')).not.toBeNull();
    expect(section.textContent).toContain('Takes effect the next time Dyno starts');
    const type = (i: HTMLInputElement, v: string) => {
      i.value = v;
      i.dispatchEvent(new Event('input'));
      f.detectChanges();
    };
    type(field('File finder: folder depth'), '0');
    type(field('File finder: files looked at'), '999');
    type(field('File finder: content compared'), '5000');
    expect(section.querySelectorAll('mat-error').length).toBe(3);
    expect(saveButton()!.disabled).toBe(true);
    type(field('File finder: folder depth'), '20');
    type(field('File finder: files looked at'), '100000');
    type(field('File finder: content compared'), '8');
    const page = f.componentInstance as unknown as { setDisk(p: object): void; setJobs(p: object): void };
    page.setDisk({ readThreads: { fixed: 6 } });
    page.setJobs({ workers: { fixed: 2 } });
    f.detectChanges();
    const spy = vi.spyOn(mock, 'saveSettings');
    saveButton()!.click();
    await settle(f);
    const sent = spy.mock.calls[0]![0];
    expect(sent.disk).toMatchObject({ readThreads: { fixed: 6 }, finderFiles: 100_000, finderDepth: 20, finderHashGib: 8 });
    expect(sent.jobs).toEqual({ workers: { fixed: 2 } });
  });
  it('ignores a learning period outside 1 to 30 and dims its rows when learning is off', async () => {
    const { f, el, saveButton } = await make();
    const input = el.querySelector<HTMLInputElement>('input[aria-label="Days of learning"]')!;
    for (const bad of ['0', '31', '']) {
      input.value = bad;
      input.dispatchEvent(new Event('input'));
      f.detectChanges();
    }
    expect(saveButton()).toBeUndefined();
    toggle(el, 'Learn what is normal').click();
    await settle(f);
    expect(input.disabled).toBe(true);
    expect(input.closest('.row')!.classList.contains('off')).toBe(true);
    expect(saveButton()).toBeDefined();
  });
});
