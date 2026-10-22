import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { EditorPage } from './editor-page';
import { ToastStore } from '../../state/toast.store';
import { SettingsStore } from '../../state/settings.store';
import { appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

@Component({ template: '' })
class Stub {}
const routes = [
  { path: 'watches', component: Stub },
  { path: 'watches/:id', component: Stub },
];

type F = Awaited<ReturnType<typeof make>>['f'];

async function make(id?: string, backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
  const f = TestBed.createComponent(EditorPage);
  if (id) f.componentRef.setInput('id', id);
  await settle(f);
  return { f, backend, router: TestBed.inject(Router) };
}
const q = <T extends Element = HTMLElement>(f: F, sel: string) => f.nativeElement.querySelector(sel) as T;
const click = async (f: F, el: Element | null) => {
  (el as HTMLElement).click();
  await settle(f);
};
const button = (f: F, label: string | RegExp) => byText(f.nativeElement, 'button', label) as HTMLButtonElement;
const check = async (f: F, el: HTMLInputElement, on: boolean) => {
  el.checked = on;
  el.dispatchEvent(new Event('change', { bubbles: true }));
  await settle(f);
};
const fill = async (f: F, sel: string, value: string, event = 'input') => {
  typeInto(q(f, sel), value, event);
  await settle(f);
};
const card = (f: F, label: string) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('button.card')).find((b) => text(b.querySelector('.t')).startsWith(label))!;

/** Adds a GitHub watch through the Source kinds tab and stops on the Configure step. */
async function startGithub(f: F, repo = 'owner/tool') {
  await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
  await click(f, card(f, 'GitHub'));
  await fill(f, '#f-repo', repo);
}
const goNext = async (f: F, times = 1) => {
  for (let i = 0; i < times; i++) await click(f, button(f, 'Next'));
};

describe('EditorPage: adding a watch', () => {
  afterEach(() => vi.restoreAllMocks());

  it('starts on the source step with presets, no Next button and later steps disabled', async () => {
    const { f } = await make();
    expect(text(q(f, 'h1'))).toBe('Add a watch');
    expect(card(f, 'Node.js')).toBeDefined();
    expect(button(f, 'Next')).toBeNull();
    const steps = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('ol.steps button'));
    expect(steps.map((s) => s.disabled)).toEqual([false, true, true, true, true]);
    expect(steps[0]!.getAttribute('aria-current')).toBe('step');
  });

  it('picks a preset, keeping the user default interval, and lands on the configure step', async () => {
    const { f } = await make();
    await click(f, card(f, 'Rust toolchain'));
    expect(text(q(f, '#cfg-h'))).toBe('GitHub');
    expect((q(f, '#f-repo') as HTMLInputElement).value).toBe('rust-lang/rust');
    expect(byText(f.nativeElement, 'p', 'Still needed')).toBeNull();
  });

  it('uses the settings default interval for a new watch', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    const settings = TestBed.inject(SettingsStore);
    await settings.loadSettings();
    await settings.change({ defaultIntervalSecs: 900 });
    const f = TestBed.createComponent(EditorPage);
    await settle(f);
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    await click(f, card(f, 'GitHub'));
    await fill(f, '#f-repo', 'a/b');
    await goNext(f, 2);
    expect(q<HTMLSelectElement>(f, '#interval').value).toBe('900');
  });

  it('groups the source kinds and walks the whole wizard to a saved watch', async () => {
    const { f, backend, router } = await make();
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    expect(f.nativeElement.querySelectorAll('h3.group').length).toBeGreaterThan(1);
    await click(f, card(f, 'GitHub'));

    // Configure: Next waits for the required field, and says what is missing.
    expect(button(f, 'Next').disabled).toBe(true);
    expect(text(byText(f.nativeElement, 'p', 'Still needed'))).toBe('Still needed: Repository.');
    expect(text(q(f, 'sp-preview-panel'))).toContain('Fill in the required fields');
    expect(button(f, 'Test again').disabled).toBe(true);
    await fill(f, '#f-repo', 'owner/tool');
    expect(button(f, 'Next').disabled).toBe(false);
    expect(byText(f.nativeElement, 'p', 'Still needed')).toBeNull();

    // The live test shows what was found.
    await click(f, button(f, 'Test again'));
    expect(text(q(f, 'sp-preview-panel'))).toMatch(/\d+ releases? found in/);
    expect(q(f, 'sp-preview-panel table').getAttribute('aria-label')).toBe('Releases found');
    expect(text(q(f, 'sp-preview-panel details'))).toContain('2 names held no version');
    expect(backend.calls).toContain('testSource');

    // Versions
    await goNext(f);
    expect(text(q(f, '#ver-h'))).toBe('Versions and filters');
    expect(q<HTMLSelectElement>(f, '#scheme').selectedOptions[0]!.textContent).toContain('Semantic version');
    await check(f, q(f, '.check input'), true);
    await fill(f, '#prefix', '1.');
    await fill(f, '#names', '*-setup.exe');
    await fill(f, '#soak', '12');
    await fill(f, '#soak', 'abc');

    // Notify
    await goNext(f);
    expect(q<HTMLSelectElement>(f, '#interval').selectedOptions[0]!.textContent).toBe('6 hours');
    await fill(f, '#interval', String(24 * 3600), 'change');
    expect(q<HTMLSelectElement>(f, '#interval').selectedOptions[0]!.textContent).toBe('Daily');
    await fill(f, '#minutes', '45', 'change');
    expect(q<HTMLSelectElement>(f, '#interval').selectedOptions[0]!.textContent).toBe('every 45 min');
    await fill(f, '#minutes', '0', 'change'); // below a minute: ignored
    expect(q<HTMLSelectElement>(f, '#interval').selectedOptions[0]!.textContent).toBe('every 45 min');
    await fill(f, '#mode', 'newer-than-installed', 'change');

    // Review
    await goNext(f);
    expect((q(f, '#name') as HTMLInputElement).value).toBe('tool');
    expect(text(q(f, 'dl'))).toContain('GitHub: owner/tool');
    expect(text(q(f, 'dl'))).toContain('every 45 min');
    expect(button(f, 'Next')).toBeNull();
    await click(f, button(f, 'Add watch'));
    await flush(5);
    expect(router.url).toBe('/watches/w-100');
    expect(TestBed.inject(ToastStore).toasts().at(-1)).toMatchObject({ title: 'Watch added', body: 'tool' });
    const saved = (await backend.getWatch('w-100')).spec;
    expect(saved).toMatchObject({ includePrereleases: true, nameFilter: '*-setup.exe', intervalSecs: 2700, notify: { mode: 'newer-than-installed' } });
    expect(saved.filter).toEqual({ prefix: '1.', soakHours: 0 });
  });

  it('needs a custom pattern before moving on from versions', async () => {
    const { f } = await make();
    await startGithub(f);
    await goNext(f);
    await fill(f, '#scheme', 'custom', 'change');
    expect(button(f, 'Next').disabled).toBe(true);
    await fill(f, '#pattern', '^v(?P<major>\\d+)');
    expect(button(f, 'Next').disabled).toBe(false);
  });

  it('lets the user go back and revisit visited steps only', async () => {
    const { f } = await make();
    await startGithub(f);
    await goNext(f, 2);
    const steps = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('ol.steps button'));
    expect(steps().map((s) => s.disabled)).toEqual([false, false, false, false, true]);
    await click(f, steps()[1]!);
    expect(q(f, '#cfg-h')).not.toBeNull();
    await click(f, button(f, 'Back'));
    expect(q(f, 'sp-source-step')).not.toBeNull();
    expect(button(f, 'Back')).toBeNull();
    f.componentInstance['go']('review', 4); // not reachable
    await settle(f);
    expect(q(f, '#rev-h')).toBeNull();
  });

  it('manages notification channels, quiet hours and actions', async () => {
    const { f, backend } = await make();
    await startGithub(f);
    await goNext(f, 2);

    // Channels
    await click(f, button(f, 'Add a place to send to'));
    await fill(f, 'input[aria-label="Channel 1 address"]', 'topic-one');
    await fill(f, 'select[aria-label="Channel 1 kind"]', 'webhook', 'change');
    await click(f, button(f, 'Add a place to send to'));
    expect(f.nativeElement.querySelectorAll('input[aria-label$=" address"]')).toHaveLength(2);
    await click(f, q(f, 'button[aria-label="Remove channel 1"]'));
    expect(f.nativeElement.querySelectorAll('input[aria-label$=" address"]')).toHaveLength(1);
    expect((q(f, 'input[aria-label="Channel 1 address"]') as HTMLInputElement).value).toBe('');

    // Quiet hours
    const labelled = (label: string) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLLabelElement>('label.check')).find((l) => text(l).includes(label))!.querySelector('input')!;
    await check(f, labelled('Quiet hours'), true);
    expect((q(f, '#qs') as HTMLInputElement).value).toBe('22:00');
    await fill(f, '#qe', '06:15', 'change');
    expect((q(f, '#qe') as HTMLInputElement).value).toBe('06:15');
    await check(f, labelled('Quiet hours'), false);
    expect(q(f, '#qs')).toBeNull();
    await check(f, labelled('Keep reminding'), true);

    // Actions
    await check(f, labelled('Download it'), true);
    await fill(f, '#dl-folder', '/tmp/spotter-test');
    await click(f, byText(f.nativeElement, 'button', 'Browse…'));
    expect((q(f, '#dl-folder') as HTMLInputElement).value).toBe('/home/demo/Downloads/Spotter-demo');
    await check(f, labelled('Verify the checksum'), false);
    await check(f, labelled('Run a program'), true);
    expect(text(q(f, '.warn'))).toContain('does not go through a shell');
    await fill(f, '#cmd-prog', '/opt/tool/run');
    await fill(f, '#cmd-args', '--flag\n\n{path}\n');
    const browses = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('button')).filter((b) => text(b) === 'Browse…');
    await click(f, browses[1]!);
    expect((q(f, '#cmd-prog') as HTMLInputElement).value).toBe('/usr/local/bin/installer');
    await check(f, labelled('Offer to open'), false);

    await goNext(f);
    await fill(f, '#name', 'Channel test');
    await click(f, button(f, 'Add watch'));
    await flush(5);
    const spec = (await backend.getWatch('w-100')).spec;
    expect(spec.channels).toEqual([{ kind: 'ntfy', target: '' }]);
    expect(spec.notify.nag).toBe(true);
    expect(spec.notify.quietHours).toBeNull();
    expect(spec.actions).toEqual([
      { kind: 'download', folder: '/home/demo/Downloads/Spotter-demo', verify: false },
      { kind: 'run-command', program: '/usr/local/bin/installer', args: ['--flag', '{path}'] },
    ]);
  });

  it('ignores a cancelled folder or program picker', async () => {
    const { f, backend } = await make();
    await startGithub(f);
    await goNext(f, 2);
    const labelled = (label: string) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLLabelElement>('label.check')).find((l) => text(l).includes(label))!.querySelector('input')!;
    await check(f, labelled('Download it'), true);
    await check(f, labelled('Run a program'), true);
    vi.spyOn(backend, 'chooseFolder').mockResolvedValue(null);
    vi.spyOn(backend, 'chooseProgram').mockRejectedValue(new Error('x'));
    const browses = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('button')).filter((b) => text(b) === 'Browse…');
    await click(f, browses[0]!);
    await click(f, browses[1]!);
    expect((q(f, '#dl-folder') as HTMLInputElement).value).toBe('');
    expect((q(f, '#cmd-prog') as HTMLInputElement).value).toBe('');
  });

  it('keeps Add watch disabled without a name and shows the engine problems when refused', async () => {
    const { f, router } = await make();
    await startGithub(f);
    await goNext(f, 3);
    await fill(f, '#name', '   ');
    expect(button(f, 'Add watch').disabled).toBe(true);
    await fill(f, '#name', 'Acme Installer');
    await click(f, button(f, 'Add watch'));
    expect(text(q(f, '.problems'))).toContain('A watch named "Acme Installer" already exists.');
    expect(router.url).not.toContain('w-100');
    await check(f, q(f, '.check input'), true);
    expect(q(f, '.problems')).toBeNull();
  });
});

describe('EditorPage: editing a watch', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads the watch, starts at configure with no way back to the source and saves changes', async () => {
    const { f, backend, router } = await make('w-2');
    expect(text(q(f, 'h1'))).toBe('Edit watch');
    expect(text(q(f, '#cfg-h'))).toBe('GitHub');
    expect((q(f, '#f-repo') as HTMLInputElement).value).toBe('lumen-labs/lumen');
    expect(button(f, 'Back')).toBeNull();
    const first = (f.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('ol.steps button')!;
    await click(f, first);
    expect(q(f, '#cfg-h')).not.toBeNull();
    await fill(f, '#f-repo', 'lumen-labs/lumen2');
    await goNext(f, 3);
    expect((q(f, '#name') as HTMLInputElement).value).toBe('Lumen CLI');
    await click(f, button(f, 'Save changes'));
    await flush(5);
    expect(router.url).toBe('/watches/w-2');
    expect(TestBed.inject(ToastStore).toasts().at(-1)?.title).toBe('Watch saved');
    expect((await backend.getWatch('w-2')).spec.source.values['repo']).toBe('lumen-labs/lumen2');
  });

  it('offers Retry when the watch cannot be loaded, and loads it then', async () => {
    const backend = makeMock();
    const real = backend.getWatch.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'getWatch').mockImplementation((id) => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real(id)));
    const { f } = await make('w-2', backend);
    expect(text(f.nativeElement)).toContain('Could not load the watch');
    expect(text(f.nativeElement)).toContain('Engine is down.');
    fail = false;
    await click(f, button(f, /retry/i));
    expect(text(q(f, '#cfg-h'))).toBe('GitHub');
  });

  it('shows the existing secrets as stored and removes one on request', async () => {
    const backend = makeMock();
    await backend.saveWatch({ ...(await backend.getWatch('w-2')).spec }, { token: 'x' });
    const { f } = await make('w-2', backend);
    expect((q(f, '#f-token') as HTMLInputElement).placeholder).toBe('Stored — type to replace');
    await click(f, button(f, 'Remove stored'));
    expect(button(f, 'Remove stored')).toBeNull();
    expect((q(f, '#f-token') as HTMLInputElement).placeholder).not.toContain('Stored');
  });
});

describe('EditorPage: finding a feed and importing a list', () => {
  afterEach(() => vi.restoreAllMocks());
  const tab = (f: F, label: string) => click(f, byText(f.nativeElement, '.ways button', label));

  it('finds the feeds of a page and builds a feed watch from one', async () => {
    const { f } = await make();
    await tab(f, 'Find a feed');
    await click(f, button(f, 'Find feeds')); // empty address: nothing happens
    expect(q(f, '.feeds')).toBeNull();
    (q(f, '#feed-url') as HTMLInputElement).value = 'https://site.example/blog';
    await click(f, button(f, 'Find feeds'));
    expect(f.nativeElement.querySelectorAll('.feeds li')).toHaveLength(2);
    await click(f, button(f, 'Watch this feed'));
    expect(text(q(f, '#cfg-h'))).toBe('RSS or Atom feed');
    expect((q(f, '#f-url') as HTMLInputElement).value).toBe('https://site.example/blog/releases.atom');
  });

  it('says when a page has no feeds or is not an address', async () => {
    const { f } = await make();
    await tab(f, 'Find a feed');
    (q(f, '#feed-url') as HTMLInputElement).value = 'https://nofeed.example';
    await click(f, button(f, 'Find feeds'));
    expect(text(f.nativeElement)).toContain('No feeds found');
    (q(f, '#feed-url') as HTMLInputElement).value = 'ftp://x';
    await click(f, button(f, 'Find feeds'));
    expect(text(q(f, '.error'))).toContain('That is not a web address.');
  });

  it('reads a pasted list and adds every watch', async () => {
    const { f, backend, router } = await make();
    await tab(f, 'Import a list');
    const read = () => button(f, 'Read the list');
    expect(read().disabled).toBe(true);
    await fill(f, '#import-format', 'brewfile', 'change');
    const area = q<HTMLTextAreaElement>(f, '#import-text');
    area.value = 'brew "alpha"\ncask "beta"\ntap "x/y"';
    area.dispatchEvent(new Event('input', { bubbles: true }));
    await settle(f);
    expect(read().disabled).toBe(false);
    await click(f, read());
    expect(text(q(f, '.surface p'))).toContain('2 watches ready to add , 1 line skipped .');
    expect(text(q(f, 'details'))).toContain('tap "x/y"');
    await click(f, button(f, 'Add 2 watches'));
    await flush(5);
    expect((await backend.listWatches()).map((w) => w.name)).toEqual(expect.arrayContaining(['alpha', 'beta']));
    expect(router.url).toBe('/watches');
    expect(TestBed.inject(ToastStore).toasts().at(-1)?.title).toBe('Added 2 watches');
  });

  it('stays on the page and lists what could not be added', async () => {
    const { f, router } = await make();
    await tab(f, 'Import a list');
    const area = q<HTMLTextAreaElement>(f, '#import-text');
    area.value = 'Acme Installer';
    area.dispatchEvent(new Event('input'));
    await settle(f);
    await click(f, button(f, 'Read the list'));
    expect(text(q(f, '.surface p'))).toContain('1 watch ready to add');
    await click(f, button(f, 'Add 1 watch'));
    const last = TestBed.inject(ToastStore).toasts().at(-1)!;
    expect(last.title).toBe('1 could not be added');
    expect(last.body).toContain('Acme Installer');
    expect(router.url).not.toBe('/watches');
  });

  it('shows an import error', async () => {
    const { f, backend } = await make();
    vi.spyOn(backend, 'importList').mockRejectedValue(new Error('unreadable list'));
    await tab(f, 'Import a list');
    const area = q<HTMLTextAreaElement>(f, '#import-text');
    area.value = 'x';
    area.dispatchEvent(new Event('input'));
    await settle(f);
    await click(f, button(f, 'Read the list'));
    expect(text(q(f, '.error'))).toBe('unreadable list');
  });

  it('shows a catalog failure with Retry on the presets', async () => {
    const backend = makeMock();
    const real = backend.presets.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'presets').mockImplementation(() => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real()));
    const { f } = await make(undefined, backend);
    expect(text(f.nativeElement)).toContain('Could not load the presets');
    fail = false;
    await click(f, byText(f.nativeElement, 'sp-source-step button', /retry/i));
    expect(card(f, 'Node.js')).toBeDefined();
  });
});

describe('FieldForm kinds (through the wizard)', () => {
  it('renders choice, integer, toggle, folder and secret fields from the engine specs', async () => {
    const { f, backend } = await make();
    const kinds = (await backend.sourceKinds()).map((k) => k.id);
    expect(kinds).toEqual(expect.arrayContaining(['folder', 'web-page', 'crates']));
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    // folder: a folder picker and a toggle
    await click(f, card(f, 'Folder'));
    await click(f, byText(f.nativeElement, 'sp-field-form button', 'Browse…'));
    expect((q(f, '#f-path') as HTMLInputElement).value).toBe('/home/demo/Downloads/Spotter-demo');
    const toggle = q<HTMLInputElement>(f, 'sp-field-form input[type=checkbox]');
    expect(toggle.checked).toBe(false);
    await check(f, toggle, true);
    expect(q<HTMLInputElement>(f, 'sp-field-form input[type=checkbox]').checked).toBe(true);
    // The wizard offers all the choices through the Back button back to the kinds.
    await click(f, button(f, 'Back'));
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    await click(f, card(f, 'Web page'));
    const select = q<HTMLSelectElement>(f, 'sp-field-form select');
    expect(select.selectedOptions[0]!.value).toBe(select.value);
    await fill(f, 'sp-field-form select', select.options[select.options.length - 1]!.value, 'change');
    expect(q<HTMLSelectElement>(f, 'sp-field-form select').selectedOptions[0]!.value).toBe(select.options[select.options.length - 1]!.value);
    await click(f, button(f, 'Back'));
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    await click(f, card(f, 'crates.io'));
    const days = q<HTMLInputElement>(f, '#f-days');
    expect(days.value).toBe('0');
    expect(days.min).toBe('0');
    expect(days.max).toBe('365');
    await fill(f, '#f-days', '30');
    expect(q<HTMLInputElement>(f, '#f-days').value).toBe('30');
    await click(f, button(f, 'Back'));
    await click(f, byText(f.nativeElement, '.ways button', 'Source kinds'));
    await click(f, card(f, 'GitHub'));
    const secret = q<HTMLInputElement>(f, 'sp-field-form input[type=password]');
    expect(secret.autocomplete).toBe('off');
    await fill(f, '#f-token', 'abc');
    expect(secret.value).toBe('abc');
  });
});

describe('SourceStep: search and category filter', () => {
  it('filters the preset tiles by text and category and says when nothing matches', async () => {
    const { f } = await make();
    const labels = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('button.card .t')).map((t) => text(t));
    const all = labels();
    expect(all.length).toBeGreaterThan(2);
    expect(f.nativeElement.querySelector('button.card sp-avatar')).not.toBeNull();
    await fill(f, 'input[type=search]', all[0]!.toLowerCase());
    expect(labels()).toContain(all[0]);
    expect(labels().length).toBeLessThan(all.length);
    await fill(f, 'input[type=search]', 'zzzz-nothing');
    expect(labels()).toEqual([]);
    expect(text(q(f, '.help'))).toContain('Nothing matches "zzzz-nothing"');
    await fill(f, 'input[type=search]', '');
    const cats = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('.seg-tabs[aria-label=Category] button'));
    expect(text(cats[0])).toBe('All');
    expect(cats[0]!.getAttribute('aria-pressed')).toBe('true');
    expect(cats.length).toBeGreaterThan(1);
    await click(f, cats[1]!);
    expect(cats[1]!.getAttribute('aria-pressed')).toBe('true');
    expect(labels().length).toBeGreaterThan(0);
    expect(labels().length).toBeLessThanOrEqual(all.length);
    await click(f, cats[0]!);
    expect(labels()).toEqual(all);
  });
});
