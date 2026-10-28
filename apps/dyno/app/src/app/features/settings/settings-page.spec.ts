import { type ComponentFixture, TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Notifier } from '../../shared/notifier';
import { SettingsStore } from '../../state/settings.store';
import { SettingsPage } from './settings-page';

const NOW = 1_700_000_000_000;

/** Renders, then lets the mock's promises and timers run and renders again (signals set after awaits). */
async function settle(f: ComponentFixture<unknown>) {
  f.detectChanges();
  for (let i = 0; i < 3; i++) {
    await new Promise((r) => setTimeout(r, 2));
    f.detectChanges();
  }
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
    expect(el.querySelector('h1')?.textContent).toBe('Settings');
    // Appearance, Updates and About are the shell's pages of the Settings window, not Dyno's.
    for (const name of ['Start-up', 'Measuring', 'Menu bar items', 'Alerts', 'Plugins']) expect(el.querySelector(`section[aria-label="${name}"]`)).not.toBeNull();
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
  it('menu bar items: adding one makes the page dirty and Save stores trayItems', async () => {
    const { f, el, saveButton, mock } = await make();
    expect(el.querySelectorAll('section[aria-label="Menu bar items"] li')).toHaveLength(2);
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Add item'))!.click();
    f.detectChanges();
    expect(el.querySelectorAll('section[aria-label="Menu bar items"] li')).toHaveLength(3);
    const spy = vi.spyOn(mock, 'saveSettings');
    saveButton()!.click();
    await settle(f);
    expect(spy.mock.calls[0]![0].trayItems!.items).toHaveLength(3);
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
    const { el, saveButton, store } = await make();
    const section = el.querySelector('section[aria-label="Backup health"]')!;
    expect(section).not.toBeNull();
    expect(section.textContent).toContain('never runs or manages a backup');
    expect(section.querySelectorAll('textarea')).toHaveLength(3);
    expect(section.textContent).toContain('Where restic is installed');
    toggle(el, 'Tell me when a backup is out of date').click();
    await new Promise((r) => setTimeout(r, 2));
    expect(saveButton()).toBeDefined();
    expect(store.settings()!.backup.alertWhenStale).toBe(true);
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
