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
    for (const name of ['Appearance', 'Measuring', 'Tray', 'Alerts', 'Plugins', 'About']) expect(el.querySelector(`section[aria-label="${name}"]`)).not.toBeNull();
    expect(saveButton()).toBeUndefined();
    expect(store.settings()).not.toBeNull();
  });
  it('shows plugins and About details from the engine', async () => {
    const { el } = await make();
    expect(el.querySelectorAll('.plugins li').length).toBeGreaterThan(0);
    expect(el.querySelector('.about')!.textContent).toContain('0.1.0');
    expect(el.querySelector('.about')!.textContent).toContain('macOS');
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
    expect(TestBed.inject(Notifier).toasts().at(-1)?.text).toBe('Settings saved');
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
  it('choosing a theme previews it immediately and counts as a change', async () => {
    const { f, el, saveButton, store } = await make();
    const target = store.settings()!.theme === 'dark' ? 'Light' : 'Dark';
    const radio = [...el.querySelectorAll<HTMLButtonElement>('[aria-label="Theme"] button')].find((b) => b.textContent?.includes(target))!;
    radio.click();
    f.detectChanges();
    expect(radio.getAttribute('aria-checked')).toBe('true');
    expect(document.documentElement.dataset['theme']).toBe(target.toLowerCase());
    expect(saveButton()).toBeDefined();
    document.documentElement.removeAttribute('data-theme');
  });
  it('tray metric checkboxes toggle membership', async () => {
    const { f, el, saveButton } = await make();
    const boxes = el.querySelectorAll<HTMLInputElement>('[aria-label="Tray metrics"] input');
    expect(boxes.length).toBe(5);
    boxes[0]!.click();
    f.detectChanges();
    expect(saveButton()).toBeDefined();
  });
  it('a rejected save keeps the draft and the bar and toasts the engine message', async () => {
    const { f, mock, el, saveButton } = await make();
    vi.spyOn(mock, 'saveSettings').mockRejectedValue({ code: 'badRequest', message: 'Nope.' });
    toggle(el, 'Start Dyno at login').click();
    f.detectChanges();
    saveButton()!.click();
    await settle(f);
    expect(saveButton()).toBeDefined();
    expect(TestBed.inject(Notifier).toasts().at(-1)).toMatchObject({ tone: 'bad', text: 'Nope.' });
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
});
