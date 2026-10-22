import { TestBed } from '@angular/core/testing';
import { SettingsPage } from './settings-page';
import { ThemeService } from '../../core/theme.service';
import { ClockService } from '../../core/clock.service';
import { SettingsStore } from '../../state/settings.store';
import { ToastStore } from '../../state/toast.store';
import { appProviders, byText, makeMock, NOW, settle, text, type as typeInto } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  localStorage.clear();
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  TestBed.inject(ClockService).set(NOW);
  const f = TestBed.createComponent(SettingsPage);
  await settle(f);
  return { f, backend, store: TestBed.inject(SettingsStore) };
}
const el = <T extends Element>(f: { nativeElement: HTMLElement }, sel: string) => f.nativeElement.querySelector(sel) as T;
const toggle = async (f: Parameters<typeof settle>[0], input: HTMLInputElement, on: boolean) => {
  input.checked = on;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await settle(f);
};

describe('SettingsPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('shows what the engine reports', async () => {
    const { f } = await make();
    const t = text(f.nativeElement);
    expect(t).toContain('Version 0.1.0');
    expect(t).toContain('/home/demo/.config/spotter-demo/config.toml');
    expect(t).toContain('Notifications are on.');
    expect(t).toContain('No browser available');
    expect(t).toContain('Your watches were brought over from Release Monitor.');
    expect(text(el(f, '.note.warn'))).toContain('Old mirror');
    expect(el<HTMLInputElement>(f, '#autostart').checked).toBe(false);
    expect(el<HTMLSelectElement>(f, '#interval').value).toBe(String(6 * 3600));
  });

  it('saves a toggle and a select through the store', async () => {
    const { f, backend } = await make();
    await toggle(f, el(f, '#notif'), false);
    expect((await backend.getSettings()).notifications).toBe(false);
    typeInto(el(f, '#digest'), 'daily', 'change');
    await settle(f);
    expect((await backend.getSettings()).digest).toBe('daily');
    typeInto(el(f, '#interval'), '900', 'change');
    await settle(f);
    expect((await backend.getSettings()).defaultIntervalSecs).toBe(900);
    await toggle(f, el(f, '#auto-upd'), false);
    expect((await backend.getSettings()).checkAppUpdates).toBe(false);
    await toggle(f, el(f, '#osweb'), true);
    expect((await backend.getSettings()).allowOsWebview).toBe(true);
    await toggle(f, el(f, '#autostart'), true);
    expect(el<HTMLInputElement>(f, '#autostart').checked).toBe(true);
  });

  it('changes the theme through the segmented control', async () => {
    const { f, backend } = await make();
    const dark = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('pd-segmented-control button, pd-segmented-control [role=radio]')).find((b) => text(b) === 'Dark') as HTMLElement;
    dark.click();
    await settle(f);
    expect((await backend.getSettings()).theme).toBe('dark');
    expect(TestBed.inject(ThemeService).preference()).toBe('dark');
  });

  it('turns quiet hours on with defaults, edits a time, and turns them off', async () => {
    const { f, backend } = await make();
    expect(el(f, 'input[aria-label="Quiet hours start"]')).toBeNull();
    await toggle(f, el(f, '#quiet'), true);
    expect((await backend.getSettings()).quietHours).toEqual({ start: '22:00', end: '07:00' });
    typeInto(el(f, 'input[aria-label="Quiet hours end"]'), '06:30', 'change');
    await settle(f);
    expect((await backend.getSettings()).quietHours).toEqual({ start: '22:00', end: '06:30' });
    await toggle(f, el(f, '#quiet'), false);
    expect((await backend.getSettings()).quietHours).toBeNull();
    f.componentInstance['setQuietTime']('start', '01:00'); // no window: nothing to edit
    await settle(f);
    expect((await backend.getSettings()).quietHours).toBeNull();
  });

  it('pauses notifications and shows the remaining time, then resumes', async () => {
    const { f, backend } = await make();
    (byText(f.nativeElement, 'button', '1 day') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Paused for another 1 day');
    expect(await backend.getSettings()).toMatchObject({ notificationsPausedUntilMs: NOW + 24 * 3_600_000 });
    (byText(f.nativeElement, 'button', 'Resume') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Notifications are on.');
  });

  it('downloads Chromium once and then hides the button', async () => {
    const { f } = await make();
    (byText(f.nativeElement, 'button', 'Download Chromium') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Chromium downloaded');
    expect(byText(f.nativeElement, 'button', 'Download Chromium')).toBeNull();
    expect(TestBed.inject(ToastStore).toasts().at(-1)?.title).toBe('Browser installed');
  });

  it.each([
    ['chromium-installed', 'Chromium found', 'success'],
    ['os-webview', 'OS web view (limited)', 'warning'],
    ['none', 'No browser available', 'neutral'],
  ] as const)('shows the %s renderer as "%s"', async (kind, word, tone) => {
    const backend = makeMock();
    vi.spyOn(backend, 'rendererStatus').mockResolvedValue({ kind, detail: 'Invented detail', canDownload: false });
    const { f } = await make(backend);
    expect(text(f.nativeElement)).toContain(word);
    expect(text(f.nativeElement)).toContain('Invented detail');
    expect(f.componentInstance['rendererTone']()).toBe(tone);
  });

  it('checks for updates, announces the version and installs', async () => {
    const { f, backend } = await make();
    (byText(f.nativeElement, 'button', 'Check for updates') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Version 0.2.0 is available. Faster checks');
    (byText(f.nativeElement, 'button', 'Install') as HTMLButtonElement).click();
    await settle(f);
    expect(backend.calls).toContain('installAppUpdate');
    (byText(f.nativeElement, 'button', 'Check for updates') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('You have the latest version.');
    expect(byText(f.nativeElement, 'button', 'Install')).toBeNull();
  });

  it('opens folders and chooses the downloads folder', async () => {
    const { f, backend } = await make();
    for (const label of ['Open folder', 'Open']) {
      for (const b of Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('button')).filter((x) => text(x) === label)) (b as HTMLButtonElement).click();
    }
    (byText(f.nativeElement, 'button', 'Choose…') as HTMLButtonElement).click();
    await settle(f);
    expect(backend.calls).toEqual(expect.arrayContaining(['open:config', 'open:logs', 'open:downloads', 'chooseFolder', 'saveSettings']));
    expect(text(f.nativeElement)).toContain('/home/demo/Downloads/Spotter-demo');
  });

  it('keeps the page and lists the engine problems when a save is refused', async () => {
    const { f, backend } = await make();
    vi.spyOn(backend, 'saveSettings').mockRejectedValue({ code: 'invalid', message: 'No.', problems: ['Use at least one minute.'] });
    await toggle(f, el(f, '#notif'), false);
    expect(text(el(f, '.problems'))).toBe('Use at least one minute.');
    expect(el<HTMLInputElement>(f, '#notif').checked).toBe(true);
  });

  it('shows the failure with Retry when settings cannot be read', async () => {
    const backend = makeMock();
    vi.spyOn(backend, 'getSettings').mockRejectedValueOnce({ code: 'unavailable', message: 'Engine is down.', problems: [] });
    const { f } = await make(backend);
    expect(text(f.nativeElement)).toContain('Could not load the settings');
    expect(text(f.nativeElement)).toContain('Engine is down.');
    (byText(f.nativeElement, 'button', /retry/i) as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Appearance');
  });
});
