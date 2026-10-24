import { Injector, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import {
  APPEARANCE_SOURCE,
  AppearanceStore,
  CommandService,
  ContributionRegistry,
  defineSettingsPage,
  msg,
  provideAppearance,
  provideContributions,
  provideShellCore,
  SHELL_PORTS,
} from '@decryp7/paddock-ui/shell';
import { DIALOG_REF, DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { SETTINGS_OVERVIEW_FIXTURE, SETTINGS_PROBLEM_FIXTURE } from '../generated/settings-fixtures';
import { appearanceFromSettings, appearanceToSettings } from './appearance';
import { MemorySettingsBackend } from './settings-backend';
import { SETTINGS_CONTRIBUTIONS, SETTINGS_DIALOG_ID, SHELL_SETTINGS_PAGES } from './settings.contributions';
import { PdSettingsSectionPage } from './settings-section';
import { PdSettingsWindow } from './settings-window';
import { provideShellSettings } from './provide-shell-settings';
import { settingsBed, settle, SPEC_PAGES } from './spec-helpers';

interface Bed {
  fixture: ComponentFixture<PdSettingsWindow>;
  el: HTMLElement;
  backend: MemorySettingsBackend;
  close: ReturnType<typeof vi.fn>;
}

async function open(options: { backend?: MemorySettingsBackend; page?: string; extra?: unknown[]; pages?: typeof SPEC_PAGES; restart?: () => void } = {}): Promise<Bed> {
  const backend = options.backend ?? new MemorySettingsBackend();
  const close = vi.fn();
  settingsBed({ backend, pages: options.pages, restart: options.restart, extra: [{ provide: DIALOG_REF, useValue: { close, closed: new Promise(() => undefined) } }, ...(options.extra ?? [])] });
  const fixture = TestBed.createComponent(PdSettingsWindow);
  if (options.page) fixture.componentRef.setInput('page', options.page);
  await settle();
  await fixture.whenStable();
  await settle();
  return { fixture, el: fixture.nativeElement as HTMLElement, backend, close };
}

const $ = <T extends Element = HTMLElement>(el: ParentNode, sel: string) => el.querySelector<T>(sel) as T;
const $$ = <T extends Element = HTMLElement>(el: ParentNode, sel: string) => [...el.querySelectorAll<T>(sel)];
const field = (el: ParentNode, key: string) => $(el, `pd-setting-field[data-key="${key}"]`);

async function go(bed: Bed, page: string): Promise<void> {
  $<HTMLButtonElement>(bed.el, `.nav [data-page="${page}"]`).click();
  await settle();
  await bed.fixture.whenStable();
  await settle();
}

async function type(bed: Bed, input: HTMLInputElement | HTMLTextAreaElement, value: string, leave = true): Promise<void> {
  input.value = value;
  input.dispatchEvent(new Event('input'));
  if (leave) input.dispatchEvent(new Event('blur'));
  await settle();
  await bed.fixture.whenStable();
}

describe('pd-settings-window', () => {
  it('lists the pages by group and opens at the first one', async () => {
    const { el } = await open();
    expect($$(el, '.nav .group').map((g) => g.textContent?.trim())).toEqual(['Application', 'Features']);
    expect($$(el, '.nav .item').map((b) => b.dataset['page'])).toEqual(['pd.appearance', 'sd.general', 'sd.storage']);
    expect($(el, '.item[aria-current="page"]').dataset['page']).toBe('pd.appearance');
    expect($(el, 'pd-settings-section h2').textContent).toBe('Appearance');
  });

  it('renders every field kind from the engine’s schema', async () => {
    const bed = await open({ page: 'sd.general' });
    const { el } = bed;
    expect($(el, '.item[aria-current="page"]').dataset['page']).toBe('sd.general');
    expect($<HTMLInputElement>(field(el, 'notify'), 'input[role="switch"]').checked).toBe(true);
    expect($<HTMLSelectElement>(field(el, 'style'), 'select').value).toBe('casual');
    expect($<HTMLInputElement>(field(el, 'greeting'), 'input[type="text"]').value).toBe('Hello');
    expect($<HTMLInputElement>(field(el, 'repeat'), 'input[type="number"]').value).toBe('1');
    expect(field(el, 'repeat').textContent).toContain('times');
    expect(field(el, 'volume').textContent).toContain('Needs restart');
    await go(bed, 'sd.storage');
    expect($<HTMLInputElement>(field(el, 'folder'), 'input.mono').placeholder).toContain('Folder');
    // Durations and sizes are edited in their unit.
    expect($<HTMLInputElement>(field(el, 'keep'), 'input').value).toBe('60');
    expect(field(el, 'keep').textContent).toContain('min');
    expect($<HTMLInputElement>(field(el, 'cache'), 'input').value).toBe('64');
    // The advanced list is behind "Show advanced".
    expect(field(el, 'ignore')).toBeNull();
    expect(el.textContent).toContain('1 advanced setting is hidden.');
    $<HTMLButtonElement>(el, 'pd-settings-section .link').click();
    await settle();
    await bed.fixture.whenStable();
    expect($<HTMLTextAreaElement>(field(el, 'ignore'), 'textarea').value).toBe('*.tmp');
    expect(field(el, 'ignore').textContent).toContain('Advanced');
  });

  it('applies a switch and a choice at once, typed values on leaving the control, in stored units', async () => {
    const bed = await open({ page: 'sd.general' });
    const { el, backend } = bed;
    const toggle = $<HTMLInputElement>(field(el, 'notify'), 'input[role="switch"]');
    toggle.checked = false;
    toggle.dispatchEvent(new Event('change'));
    await settle();
    expect(backend.values('sd.general')?.['notify']).toBe(false);
    expect($(el, '.foot .status').textContent).toBe('Show a notification saved.');
    const style = $<HTMLSelectElement>(field(el, 'style'), 'select');
    style.value = 'formal';
    style.dispatchEvent(new Event('change'));
    await settle();
    expect(backend.values('sd.general')?.['style']).toBe('formal');
    await type(bed, $<HTMLInputElement>(field(el, 'greeting'), 'input'), 'Hi there', false);
    expect(backend.values('sd.general')?.['greeting']).toBe('Hello');
    $<HTMLInputElement>(field(el, 'greeting'), 'input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await settle();
    expect(backend.values('sd.general')?.['greeting']).toBe('Hi there');
    await go(bed, 'sd.storage');
    await type(bed, $<HTMLInputElement>(field(el, 'keep'), 'input'), '120');
    expect(backend.values('sd.storage')?.['keep']).toBe(7_200_000);
    await type(bed, $<HTMLInputElement>(field(el, 'folder'), 'input'), '/var/demo');
    expect(backend.values('sd.storage')?.['folder']).toBe('/var/demo');
  });

  it('shows the engine’s refusal at the field, marks the page and asks before closing with unsaved edits', async () => {
    const bed = await open({ page: 'sd.general' });
    const { el, backend, close } = bed;
    await type(bed, $<HTMLInputElement>(field(el, 'greeting'), 'input'), 'Hi!');
    expect(field(el, 'greeting').querySelector('[role="alert"]')?.textContent).toBe('Does not have the expected format ([A-Za-z ,]+).');
    expect($<HTMLInputElement>(field(el, 'greeting'), 'input').getAttribute('aria-invalid')).toBe('true');
    expect($(el, '.nav [data-page="sd.general"] .dot')).not.toBeNull();
    // Another field still applies; the refused one stays unsaved.
    await type(bed, $<HTMLInputElement>(field(el, 'repeat'), 'input'), '3');
    expect(backend.values('sd.general')).toMatchObject({ repeat: 3, greeting: 'Hello' });
    // Close is held, with a question.
    $<HTMLButtonElement>(el, '.foot [data-dialog-close]').click();
    expect(bed.fixture.componentInstance.dialogCanClose()).toBe(false);
    await bed.fixture.whenStable();
    const ask = $(el, '[data-testid="settings-unsaved"]');
    expect(ask.textContent).toContain('Some changes are not saved: General.');
    $$<HTMLButtonElement>(ask, 'button')[1]!.click();
    await bed.fixture.whenStable();
    expect($(el, '[data-testid="settings-unsaved"]')).toBeNull();
    bed.fixture.componentInstance.dialogCanClose();
    await bed.fixture.whenStable();
    $$<HTMLButtonElement>($(el, '[data-testid="settings-unsaved"]'), 'button')[0]!.click();
    await bed.fixture.whenStable();
    expect(close).toHaveBeenCalled();
    expect(bed.fixture.componentInstance.dialogCanClose()).toBe(true);
  });

  it('does not send a value that fails the pre-check', async () => {
    const bed = await open({ page: 'sd.general' });
    const spy = vi.spyOn(bed.backend, 'set');
    await type(bed, $<HTMLInputElement>(bed.el.querySelector('pd-setting-field[data-key="repeat"]')!, 'input'), '42');
    expect(spy).not.toHaveBeenCalled();
    expect(field(bed.el, 'repeat').textContent).toContain('Between 1–10 times.');
  });

  it('resets a field and a section to their defaults', async () => {
    const bed = await open({ page: 'sd.general' });
    const { el, backend } = bed;
    await backend.set('sd.general', { repeat: 4, greeting: 'Yo' });
    await settle();
    await bed.fixture.whenStable();
    const reset = $<HTMLButtonElement>(field(el, 'repeat'), 'button.reset');
    expect(reset.getAttribute('aria-label')).toBe('Reset to default: Repeat');
    expect(reset.title).toBe('Default: 1 times');
    reset.click();
    await settle();
    expect(backend.values('sd.general')).toMatchObject({ repeat: 1, greeting: 'Yo' });
    $<HTMLButtonElement>(el, 'pd-settings-section .head .btn').click();
    await settle();
    expect(backend.values('sd.general')?.['greeting']).toBe('Hello');
    await bed.fixture.whenStable();
    expect($(el, '.foot .status').textContent).toBe('General reset to defaults.');
  });

  it('searches every setting however it is written, and Escape clears the search', async () => {
    const bed = await open();
    const { el } = bed;
    const search = $<HTMLInputElement>(el, '[data-testid="settings-search"]');
    await type(bed, search, 'datafodler', false);
    expect($$(el, 'pd-setting-field').map((f) => f.dataset['key'])).toEqual(['folder']);
    await type(bed, search, 'ignored', false);
    expect($$(el, 'pd-setting-field').map((f) => f.dataset['key'])).toEqual(['ignore']);
    await type(bed, search, 'nothing like this', false);
    expect(el.textContent).toContain('No settings match “nothing like this”.');
    const esc = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true, bubbles: true });
    search.dispatchEvent(esc);
    await bed.fixture.whenStable();
    expect(esc.defaultPrevented).toBe(true);
    expect(search.value).toBe('');
    const again = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true, bubbles: true });
    search.dispatchEvent(again);
    expect(again.defaultPrevented).toBe(false);
  });

  it('finds a page without a section by its title', async () => {
    const custom = defineSettingsPage({ id: 'sd.keys', group: 'application', title: msg('sd.keys', 'Keyboard shortcuts'), order: 200, load: () => Promise.resolve(PdSettingsSectionPage) });
    const bed = await open({ pages: [...SPEC_PAGES, custom] });
    await type(bed, $<HTMLInputElement>(bed.el, '[data-testid="settings-search"]'), 'keyboard', false);
    const link = $<HTMLButtonElement>(bed.el, '.page-hit .link');
    expect(link.textContent).toBe('Keyboard shortcuts');
    link.click();
    await settle();
    await bed.fixture.whenStable();
    await settle();
    expect($(bed.el, '.item[aria-current="page"]').dataset['page']).toBe('sd.keys');
    expect(bed.el.textContent).toContain('These settings are not available in this app.');
  });

  it('shows the restart bar with Restart now when the app can restart, and the start-up problem', async () => {
    const restart = vi.fn();
    const backend = new MemorySettingsBackend(SETTINGS_OVERVIEW_FIXTURE, { problem: SETTINGS_PROBLEM_FIXTURE });
    const bed = await open({ backend, page: 'sd.general', restart });
    const { el } = bed;
    expect($(el, '[data-testid="settings-problem"]').textContent).toContain('Defaults are in use');
    expect($(el, '[data-testid="settings-restart"]')).toBeNull();
    await type(bed, $<HTMLInputElement>(field(el, 'volume'), 'input'), '0.8');
    const bar = $(el, '[data-testid="settings-restart"]');
    expect(bar.textContent).toContain('1 change takes effect after the app restarts (Volume).');
    $<HTMLButtonElement>(bar, 'button').click();
    expect(restart).toHaveBeenCalled();
  });

  it('moves between pages with the arrow keys', async () => {
    const { el, fixture } = await open();
    document.body.appendChild(el);
    const items = $$<HTMLButtonElement>(el, '.nav .item');
    items[0]!.focus();
    const nav = $(el, '.nav');
    const key = (k: string) => nav.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    key('ArrowDown');
    expect(document.activeElement).toBe(items[1]);
    key('End');
    expect(document.activeElement).toBe(items[2]);
    key('ArrowDown');
    expect(document.activeElement).toBe(items[2]);
    key('ArrowUp');
    expect(document.activeElement).toBe(items[1]);
    key('Home');
    expect(document.activeElement).toBe(items[0]);
    key('a');
    expect(document.activeElement).toBe(items[0]);
    fixture.destroy();
    el.remove();
  });

  it('a page that fails to load offers Retry; a failed overview offers Retry too', async () => {
    let fail = true;
    const flaky = defineSettingsPage({
      id: 'sd.general',
      group: 'application',
      title: msg('sd.general', 'General'),
      order: 1,
      load: () => (fail ? Promise.reject(new Error('chunk missing')) : Promise.resolve(PdSettingsSectionPage)),
    });
    const bed = await open({ pages: [flaky], page: 'sd.general' });
    expect(bed.el.textContent).toContain('This page could not be opened.');
    fail = false;
    $<HTMLButtonElement>(bed.el, '.content .btn').click();
    await settle();
    await bed.fixture.whenStable();
    await settle();
    expect(field(bed.el, 'greeting')).not.toBeNull();

    TestBed.resetTestingModule();
    const broken = new MemorySettingsBackend();
    vi.spyOn(broken, 'overview').mockRejectedValueOnce(new Error('gone'));
    const again = await open({ backend: broken });
    expect(again.el.textContent).toContain('The settings could not be loaded: gone');
    $<HTMLButtonElement>(again.el, '.content .btn').click();
    await settle();
    await again.fixture.whenStable();
    await settle();
    expect($$(again.el, '.nav .item')).toHaveLength(3);
  });
});

describe('appearance values', () => {
  it('maps known values only; an empty pack is no pack', () => {
    expect(appearanceFromSettings({ theme: 'dark', contrast: 'more', motion: 'reduce', density: 'compact', pack: 'sd.theme.warm-paper' })).toEqual({
      theme: 'dark',
      contrast: 'more',
      motion: 'reduce',
      density: 'compact',
      pack: 'sd.theme.warm-paper',
    });
    expect(appearanceFromSettings({ pack: '' })).toEqual({ pack: null });
    expect(appearanceFromSettings({ theme: 'blue', density: 3, contrast: 'max', motion: true, pack: 'Not An Id' })).toEqual({});
    expect(appearanceToSettings({ theme: 'light', contrast: 'standard', motion: 'full', density: 'standard', pack: null })).toEqual({
      theme: 'light',
      contrast: 'standard',
      motion: 'full',
      density: 'standard',
      pack: '',
    });
  });
});

describe('SETTINGS_CONTRIBUTIONS and provideShellSettings', () => {
  function bed() {
    const opened: [string, unknown][] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideShellCore(),
        provideContributions(SETTINGS_CONTRIBUTIONS),
        provideShellSettings({ backend: () => new MemorySettingsBackend(), appearance: false }),
        { provide: DialogService, useValue: { open: (id: string, o: { inputs?: unknown }) => void opened.push([id, o.inputs]) } },
        { provide: SHELL_PORTS, useValue: { showPage: () => undefined, notify: () => undefined, openDialog: (id: string) => Promise.resolve(void opened.push([id, 'port'])) } },
      ],
    });
    return opened;
  }

  it('passes the contributions lint and loads the shell’s Appearance page lazily', async () => {
    bed();
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
    const registry = TestBed.inject(ContributionRegistry);
    expect(registry.has('keybindings', 'window:Mod+,:pd.settings.open')).toBe(true);
    expect(await SHELL_SETTINGS_PAGES[0]!.load()).toBe(PdSettingsSectionPage);
  });

  it('opens the Settings window from the command and at a page from a notice', async () => {
    const opened = bed();
    expect(await TestBed.inject(CommandService).run('pd.settings.open')).toBe(true);
    const openSettings = TestBed.inject(OPEN_SETTINGS);
    await openSettings('sd.storage');
    await openSettings('');
    expect(opened).toEqual([
      [SETTINGS_DIALOG_ID, 'port'],
      [SETTINGS_DIALOG_ID, { page: 'sd.storage' }],
      [SETTINGS_DIALOG_ID, {}],
    ]);
  });

  it('is the source of the window’s appearance, asked nothing before its gate opens', async () => {
    const backend = new MemorySettingsBackend();
    const listen = vi.spyOn(backend, 'listen');
    const ready = signal(false);
    const root = document.createElement('html');
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideShellSettings({ backend: () => backend, appearance: { gate: () => ready } }),
        provideAppearance({ accent: 'pitwall', root: () => root }),
      ],
    });
    const store = TestBed.inject(AppearanceStore);
    TestBed.tick();
    await settle();
    expect(listen).not.toHaveBeenCalled();
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(listen).toHaveBeenCalledTimes(1);
    ready.set(false);
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(listen).toHaveBeenCalledTimes(1);
    await backend.set('pd.appearance', { density: 'comfortable', contrast: 'more' });
    TestBed.tick();
    expect(store.density()).toBe('comfortable');
    expect(root.getAttribute('data-contrast')).toBe('more');
  });

  it('without a gate the engine is asked at once; `appearance: false` leaves the look to the app', async () => {
    const backend = new MemorySettingsBackend();
    await backend.set('pd.appearance', { theme: 'dark' });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellSettings({ backend: () => backend }), provideAppearance({ accent: 'pitwall', root: () => document.createElement('html') })] });
    const store = TestBed.inject(AppearanceStore);
    await settle();
    expect(store.theme()).toBe('dark');
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellSettings({ backend: () => backend, appearance: false })] });
    expect(TestBed.inject(APPEARANCE_SOURCE, null)).toBeNull();
    localStorage.clear();
  });
});
