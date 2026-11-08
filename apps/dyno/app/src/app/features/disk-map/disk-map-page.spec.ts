import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { formatStorage } from '../../shared/format';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Notifier } from '../../shared/notifier';
import { DiskMapStore } from '../../state/disk-map.store';
import { DiskMapPage } from './disk-map-page';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(DiskMapPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock, store: TestBed.inject(DiskMapStore) };
}
type Fixture = ReturnType<typeof render>['f'];
/** Lets pending promises and the mock's scan timers run, then renders. */
const settle = async (f: Fixture, advance = false) => {
  for (let i = 0; i < 5; i++) {
    if (advance) await vi.advanceTimersByTimeAsync(0);
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text));
const text = (el: HTMLElement, sel: string) => el.querySelector(sel)?.textContent?.replace(/\s+/g, ' ').trim();
const names = (el: HTMLElement) => [...el.querySelectorAll('.rows .item .name')].map((n) => n.textContent?.trim());

/** Scans the internal volume to the end. */
async function scanned(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const r = render(options);
  await settle(r.f);
  btn(r.el, 'Scan Macintosh HD')!.click();
  await settle(r.f);
  await settle(r.f, true);
  return r;
}

describe('dy-disk-map-page', () => {
  // The mock's scan steps are timers; faking them lets a test decide when the scan moves (no sleeps).
  beforeEach(() => {
    vi.useFakeTimers({ now: NOW, toFake: ['setTimeout', 'clearTimeout', 'Date'] });
    // jsdom has no layout: the chart only needs the observer to exist.
    vi.stubGlobal('ResizeObserver', class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  });
  afterEach(() => vi.useRealTimers());

  it('offers to scan the internal volume when nothing was scanned, with one primary and no header action', async () => {
    const { f, el } = render();
    await settle(f);
    expect(text(el, 'h1')).toBe('Disk map');
    expect(text(el, '.page-head p')).toBe('Macintosh HD · APFS · internal');
    expect(text(el, '[aria-label="Not scanned"] h2')).toBe('See what fills Macintosh HD');
    expect(el.querySelectorAll('button.mdc-button--unelevated')).toHaveLength(1);
    expect(text(el, '.capacity .cap-line')).toContain('used of');
    expect(el.querySelector('.job')).toBeNull();
    expect(btn(el, 'Largest files')).toBeUndefined();
    expect(el.textContent).not.toContain('Duplicate');
  });

  it('shows a skeleton while the stored result is read, then the result without a flash of the empty state', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const real = mock.diskScans.bind(mock);
    vi.spyOn(mock, 'diskScans').mockImplementation(async () => {
      await gate;
      return real();
    });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(DiskMapPage);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    await settle(f);
    expect(el.querySelector('[aria-label="Loading the last scan"]')!.getAttribute('aria-busy')).toBe('true');
    expect(el.querySelector('[aria-label="Loading the last scan"]')!.textContent!.trim()).toBe('');
    expect(el.querySelector('[aria-label="Not scanned"]')).toBeNull();
    expect(el.querySelector('.capacity')).toBeNull();
    release();
    await settle(f);
    expect(el.querySelector('[aria-label="Loading the last scan"]')).toBeNull();
    expect(el.querySelector('[aria-label="Not scanned"]')).not.toBeNull();
    expect(el.querySelector('.capacity')).not.toBeNull();
  });

  it('shows a scanning job with a phase, a growing count and Cancel, and no Rescan', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'Scan Macintosh HD')!.click();
    await settle(f);
    const job = el.querySelector('.job')!;
    expect(job.getAttribute('aria-label')).toBe('Scan progress');
    expect(text(el, '.job-title')).toContain('Reading volume facts');
    expect(el.querySelector('mat-progress-bar')!.getAttribute('role')).toBe('progressbar');
    expect(btn(el, 'Cancel scan')).toBeDefined();
    expect(btn(el, 'Rescan')).toBeUndefined();
    expect(el.querySelector('.picker')!.hasAttribute('disabled')).toBe(true);
    await settle(f, true);
    expect(el.querySelector('.job')).toBeNull();
  });

  it('shows the finished volume: list, capacity strip with hidden space, and Rescan as the one primary', async () => {
    const { el } = await scanned();
    expect(text(el, '.page-head p')).toMatch(/^Macintosh HD · APFS · internal · scanned /);
    expect(names(el)[0]).toBe('Users');
    expect(text(el, '#in-folder')).toMatch(/^In this folder \(\d+\)$/);
    expect(text(el, '.legend')).toContain('Hidden space');
    expect(text(el, '.legend')).not.toContain('Not counted yet');
    const primary = el.querySelectorAll('button.mdc-button--unelevated');
    expect(primary).toHaveLength(1);
    expect(primary[0]!.textContent).toContain('Rescan Macintosh HD');
    // Only the 1.0 tab exists.
    expect(el.textContent).not.toMatch(/Largest files|Oldest files|Duplicate hints/);
    expect(el.querySelector('[role="tablist"]')).toBeNull();
  });

  it('drills in with a click, shows the breadcrumb, and goes up with Go up, the crumb and Alt+Up', async () => {
    const { f, el } = await scanned();
    const open = (name: string) => [...el.querySelectorAll<HTMLButtonElement>('.rows .name.link')].find((b) => b.textContent?.includes(name))!.click();
    open('Users');
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Users');
    expect(el.querySelector('.centre h2')).toBeNull();
    expect(el.querySelectorAll('button[aria-label^="Go up"]')).toHaveLength(1);
    expect(btn(el, 'Go up')!.getAttribute('aria-label')).toBe('Go up to Macintosh HD');
    open('alex');
    await settle(f);
    expect([...el.querySelectorAll('.crumbs li')].map((l) => l.textContent?.replace(/[›\s]+$/, '').trim())).toEqual(['Macintosh HD', 'Users', 'alex']);
    el.querySelector<HTMLButtonElement>('.toolbar button[aria-label^="Go up"]')!.click();
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Users');
    open('alex');
    await settle(f);
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, bubbles: true }));
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Users');
    el.querySelector<HTMLButtonElement>('.crumb.link')!.click();
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Macintosh HD');
    expect(el.querySelector('.toolbar button[aria-label^="Go up"]')!.hasAttribute('disabled')).toBe(true);
  });

  it('goes to the root with Ctrl+Home and ignores the keys while typing in the filter', async () => {
    const { f, el } = await scanned();
    [...el.querySelectorAll<HTMLButtonElement>('.rows .name.link')].find((b) => b.textContent?.includes('Users'))!.click();
    await settle(f);
    btn(el, 'Table')!.click();
    await settle(f);
    const input = el.querySelector<HTMLInputElement>('input[type="search"]')!;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', ctrlKey: true, bubbles: true }));
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Users');
    el.querySelector('table')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', ctrlKey: true, bubbles: true }));
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Macintosh HD');
  });

  it('counts the rows it shows: the list heading and the table toolbar agree with their rows, folded small items included', async () => {
    const { f, el } = await scanned();
    const listed = el.querySelectorAll('.rows .item').length;
    expect(text(el, '#in-folder')).toBe(`In this folder (${listed})`);
    btn(el, 'Table')!.click();
    await settle(f);
    const shown = el.querySelectorAll('tbody tr.mat-mdc-row').length + el.querySelectorAll('tfoot tr:not(.foot-off)').length;
    expect(text(el, '.table-tools .small')).toBe(`${shown} of ${shown} rows`);
  });

  it('switches to the table, which carries every column, sorts and filters it, and keeps the centre', async () => {
    const { f, el } = await scanned();
    [...el.querySelectorAll<HTMLButtonElement>('.rows .name.link')].find((b) => b.textContent?.includes('Users'))!.click();
    await settle(f);
    btn(el, 'Table')!.click();
    await settle(f);
    expect(el.querySelector('pd-segmented-control .mat-button-toggle-checked')!.textContent?.trim()).toBe('Table');
    expect([...el.querySelectorAll('th')].map((h) => h.textContent?.trim().replace(/\s*[↑↓]$/, ''))).toEqual(['Name', 'Size', 'Items', 'Newest change', 'Share', 'Actions']);
    expect(text(el, '.crumb.current')).toBe('Users');
    const firstNames = () => [...el.querySelectorAll('tbody tr td.nm .name')].map((n) => n.textContent?.trim());
    expect(el.querySelector('th[aria-sort="descending"]')!.textContent).toContain('Size');
    [...el.querySelectorAll<HTMLElement>('th')].find((h) => h.textContent?.includes('Name'))!.querySelector<HTMLElement>('[role="button"], button')!.click();
    await settle(f);
    expect(el.querySelector('th[aria-sort="ascending"]')!.textContent).toContain('Name');
    [...el.querySelectorAll<HTMLElement>('th')].find((h) => h.textContent?.includes('Name'))!.querySelector<HTMLElement>('[role="button"], button')!.click();
    await settle(f);
    expect(el.querySelector('th[aria-sort="descending"]')!.textContent).toContain('Name');
    const input = el.querySelector<HTMLInputElement>('input[type="search"]')!;
    input.value = 'zzz';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(text(el, 'tbody')).toContain('No row matches "zzz".');
    input.value = 'ale';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(firstNames()).toEqual(['alex']);
    btn(el, 'Chart')!.click();
    await settle(f);
    expect(text(el, '.crumb.current')).toBe('Users');
    expect(el.querySelector('table')).toBeNull();
  });

  it('marks a cancelled scan as a partial result that is still usable, and offers Rescan', async () => {
    const r = render();
    await settle(r.f);
    btn(r.el, 'Scan Macintosh HD')!.click();
    await settle(r.f);
    btn(r.el, 'Cancel scan')!.click();
    await settle(r.f);
    await settle(r.f, true);
    expect(text(r.el, '.callout.info')).toContain('Partial result.');
    expect(text(r.el, '.callout.info')).toContain('Stopped by you');
    expect(r.el.querySelector('.rows')).not.toBeNull();
    expect(r.el.querySelector('button.mdc-button--unelevated')!.textContent).toContain('Rescan');
    expect(text(r.el, '.legend')).toContain('Not counted yet');
    expect(text(r.el, '.legend')).not.toContain('Hidden space');
  });

  it('says a network share stopped answering, keeps what was counted and retries', async () => {
    const r = render();
    await settle(r.f);
    r.store.selectVolume('/Volumes/Media');
    await settle(r.f);
    btn(r.el, 'Scan Media')!.click();
    await settle(r.f);
    await settle(r.f, true);
    const callout = r.el.querySelector('.callout.bad')!;
    expect(callout.getAttribute('data-kind')).toBe('network');
    expect(callout.textContent).toContain('Scan stopped: nas.local stopped answering');
    expect(callout.textContent).toContain('What was counted is kept');
    expect(r.el.querySelector('.rows')).not.toBeNull();
    expect(r.el.querySelector('.page-head button.mdc-button--unelevated')).toBeNull();
    expect(text(r.el, '.capacity')).toContain('server volume');
    btn(callout as HTMLElement, 'Retry scan')!.click();
    await settle(r.f);
    expect(r.el.querySelector('.job')).not.toBeNull();
  });

  it('explains unreadable folders without hiding the result', async () => {
    const { el } = await scanned();
    expect(text(el, '.callout.warn')).toContain('1 folder could not be read.');
    expect(text(el, '.callout.warn')).toContain('/Users/alex/Library/Mail');
  });

  it('shows the Full Disk Access state when the engine says nothing could be read, and opens the pane', async () => {
    const r = render();
    await settle(r.f);
    btn(r.el, 'Scan Macintosh HD')!.click();
    await settle(r.f);
    r.store.receive({ ...r.store.status()!, seq: 50, state: 'done', needsFullDiskAccess: true, progress: { bytes: 0, items: 0, folders: 3, totalBytes: null, unreadable: 3 } });
    await settle(r.f);
    const card = r.el.querySelector('[aria-label="Nothing readable"]')!;
    expect(card.querySelector('h2')!.textContent).toBe("Dyno can't read Macintosh HD");
    expect(card.querySelectorAll('ol li')).toHaveLength(3);
    expect(r.el.querySelector('.callout.warn')).toBeNull();
    expect(card.querySelectorAll('button.mdc-button--unelevated')).toHaveLength(1);
    const open = vi.spyOn(r.mock, 'openFullDiskAccessSettings');
    btn(card as HTMLElement, 'Open Full Disk Access settings')!.click();
    await settle(r.f);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('offers the pane and Read again on a partly unreadable result, and reads only those folders again', async () => {
    const r = await scanned();
    const warn = r.el.querySelector('.callout.warn') as HTMLElement;
    expect(btn(warn, 'Open Full Disk Access settings')).toBeDefined();
    const retry = vi.spyOn(r.mock, 'retryDiskScan');
    const scan = vi.spyOn(r.mock, 'startDiskScan');
    btn(warn, 'Read again')!.click();
    await settle(r.f);
    expect(retry).toHaveBeenCalledWith(r.store.status()!.scan);
    expect(scan).not.toHaveBeenCalled();
    expect(r.el.querySelector('.job')).not.toBeNull();
    await settle(r.f, true);
    expect(r.el.querySelector('.job')).toBeNull();
    expect(r.store.outcome()).toBe('done');
  });

  it('shows the engine hidden-space figure and nothing when it sends none', async () => {
    const r = await scanned();
    expect(text(r.el, '.legend')).toContain('Hidden space');
    r.store.receive({ ...r.store.status()!, seq: 99, hiddenBytes: null });
    await settle(r.f);
    expect(text(r.el, '.legend')).not.toContain('Hidden space');
  });

  it('shows the rate and time left only when known, and how long ago a finished scan was', async () => {
    const r = render();
    await settle(r.f);
    btn(r.el, 'Scan Macintosh HD')!.click();
    await settle(r.f);
    expect(text(r.el, '.job-count')).not.toContain('items/s');
    r.store.receive({ ...r.store.status()!, seq: 60, itemsPerSecond: 1850, etaSeconds: 40 });
    await settle(r.f);
    expect(text(r.el, '.job-count')).toMatch(/1,850 items\/s · about .* left/);
    r.store.receive({ ...r.store.status()!, seq: 61, state: 'done', scannedAt: NOW - 120_000 });
    await settle(r.f);
    expect(text(r.el, '.page-head p')).toBe('Macintosh HD · APFS · internal · scanned 2 min ago');
    r.store.receive({ ...r.store.status()!, seq: 62, scannedAt: NOW + 5_000 });
    await settle(r.f);
    expect(text(r.el, '.page-head p')).toBe('Macintosh HD · APFS · internal · scanned just now');
  });

  it('reveals the folder in the file manager from the centre, the row and the menu', async () => {
    const r = await scanned();
    const reveal = vi.spyOn(r.mock, 'diskReveal');
    expect(btn(r.el, 'Show in folder')).toBeDefined();
    btn(r.el, 'Show in folder')!.click();
    await settle(r.f);
    r.el.querySelector<HTMLButtonElement>('.rows button[mat-icon-button], .rows button.mat-mdc-icon-button')!.click();
    await settle(r.f);
    const scan = r.store.status()!.scan;
    expect(reveal).toHaveBeenNthCalledWith(1, scan, r.store.level()!.node.id);
    expect(reveal).toHaveBeenNthCalledWith(2, scan, r.store.level()!.children[0]!.id);
  });

  it('shows an empty folder with a way out', async () => {
    const r = await scanned();
    const real = r.mock.diskChildren.bind(r.mock);
    vi.spyOn(r.mock, 'diskChildren').mockImplementation(async (q) => {
      const page = await real(q);
      return q.node === null ? page : { ...page, children: [], other: { count: 0, bytes: 0, logicalBytes: 0, items: 0 }, loose: { count: 0, bytes: 0, logicalBytes: 0, items: 0 }, shown: 0 };
    });
    [...r.el.querySelectorAll<HTMLButtonElement>('.rows .name.link')].find((b) => b.textContent?.includes('Users'))!.click();
    await settle(r.f);
    expect(text(r.el, '[aria-label="Empty folder"] h2')).toBe('This folder is empty');
    expect(btn(r.el.querySelector('[aria-label="Empty folder"]') as HTMLElement, 'Go up')).toBeUndefined();
    r.el.querySelector<HTMLButtonElement>('.toolbar button[aria-label^="Go up"]')!.click();
    await settle(r.f);
    expect(r.el.querySelector('[aria-label="Empty folder"]')).toBeNull();
    expect(text(r.el, '.crumb.current')).toBe('Macintosh HD');
  });

  it('lists volumes by kind in the picker, marks a share that does not answer, and switches volume', async () => {
    const r = render();
    await settle(r.f);
    r.el.querySelector<HTMLButtonElement>('.picker')!.click();
    await settle(r.f);
    const menu = document.querySelector('[aria-label="Volumes"]')!;
    expect([...menu.querySelectorAll('.menu-label')].map((l) => l.textContent?.trim().split(' · ')[0])).toEqual(['Internal', 'External', 'Network']);
    expect(menu.textContent).toContain('Slower to scan; a dropped link stops the scan.');
    expect(menu.textContent).toContain('SMB network volume');
    expect([...menu.querySelectorAll('[cdkMenuItem], button')].some((b) => b.textContent?.includes('Choose a folder…'))).toBe(true);
    [...menu.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Photos'))!.click();
    await settle(r.f);
    expect(text(r.el, '.page-head p')).toBe('Photos · EXFAT · USB');
    expect(text(r.el, '[aria-label="Not scanned"] h2')).toBe('See what fills Photos');
  });

  it('flags a share that did not answer and does not start its scan', async () => {
    const r = render();
    await settle(r.f);
    vi.spyOn(r.mock, 'diskVolumes').mockResolvedValue((await r.mock.diskVolumes()).map((v) => (v.name === 'Media' ? { ...v, reachable: false } : v)));
    await r.store.loadVolumes();
    await r.store.selectVolume('/Volumes/Media');
    await settle(r.f);
    expect(btn(r.el, 'Scan Media')!.hasAttribute('disabled')).toBe(true);
    expect(r.el.textContent).toContain('Media is not answering');
  });

  it('copies the path of a row and says so', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    const { f, el } = await scanned();
    el.querySelector<HTMLButtonElement>('.rows button[aria-label^="Copy path"]')!.click();
    await settle(f);
    expect(write).toHaveBeenCalledWith('/Users');
    btn(el, 'Copy path')!.click();
    await settle(f);
    expect(write).toHaveBeenCalledTimes(2);
    expect(TestBed.inject(Notifier).recent().at(-1)!.text).toBe('Path copied');
    write.mockRejectedValueOnce(new Error('denied'));
    btn(el, 'Copy path')!.click();
    await settle(f);
    expect(TestBed.inject(Notifier).recent().at(-1)).toMatchObject({ tone: 'bad', text: expect.stringContaining('could not be copied') });
  });

  it('shows the engine message when the volumes cannot be read, with Retry', async () => {
    const { f, el } = render({ fail: new Set(['diskVolumes']) });
    await settle(f);
    expect(el.querySelector('dy-async')!.textContent).toContain('The mock was asked to fail');
    expect(el.querySelector('.toolbar')).toBeNull();
  });

  const arc = (el: HTMLElement, id: string) => el.querySelector<SVGPathElement>(`pd-sunburst path[data-id="${id}"]`)!;
  const click = (e: Element) => e.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  const idOf = (r: { store: InstanceType<typeof DiskMapStore> }, name: string) => r.store.rings()!.children.find((c) => c.node.name === name)!.node.id;

  it('draws the ring beside the list, with hidden space as a segment and the list in step with it', async () => {
    const r = await scanned();
    const tree = r.el.querySelector('pd-sunburst svg[role="tree"]')!;
    expect(tree.getAttribute('aria-label')).toBe('Disk usage of Macintosh HD');
    expect(arc(r.el, `n${idOf(r, 'Users')}`).getAttribute('role')).toBe('treeitem');
    expect(arc(r.el, 'hidden')).not.toBeNull();
    expect(arc(r.el, 'pending')).toBeNull();
    expect(r.el.querySelectorAll('pd-sunburst path[role="treeitem"]').length).toBeGreaterThan(5);
  });

  it('shows the ghost for what is not counted yet while a scan runs, without animation', async () => {
    const r = render();
    await settle(r.f);
    btn(r.el, 'Scan Macintosh HD')!.click();
    // Whether the mock's scan steps (faked timers) had run before the status below was pushed used to depend on timing; when
    // they had, the copied status was the finished one and carried its hidden space. Let the scan finish first, so every run
    // takes the same path, and push a running status that says what a running scan says: no hidden space, no scan time.
    await vi.waitFor(async () => {
      await settle(r.f);
      expect(r.store.status()?.state).toBe('done');
    });
    r.store.receive({ ...r.store.status()!, seq: 70, state: 'running', hiddenBytes: null, scannedAt: null });
    // The sunburst draws after the status arrives: wait for the ghost arc, not for a fixed number of ticks.
    await vi.waitFor(async () => {
      await settle(r.f);
      expect(arc(r.el, 'pending')).not.toBeNull();
      expect(arc(r.el, 'hidden')).toBeNull();
      expect(r.el.querySelector('pd-sunburst .pd-fade')).toBeNull();
    });
  });

  it('drills into a folder when its segment is clicked, and the centre goes up again', async () => {
    const r = await scanned();
    click(arc(r.el, `n${idOf(r, 'Users')}`));
    await settle(r.f);
    expect(text(r.el, '.crumb.current')).toBe('Users');
    expect(names(r.el)[0]).toBe('alex');
    const alex = r.store.level()!.children.find((c) => c.name === 'alex')!;
    click(arc(r.el, `n${alex.id}`));
    await settle(r.f);
    expect(text(r.el, '.crumb.current')).toBe('alex');
    r.el.querySelector<HTMLButtonElement>('pd-sunburst .pd-up')!.click();
    await settle(r.f);
    expect(text(r.el, '.crumb.current')).toBe('Users');
    expect(r.el.querySelector('.toolbar button[aria-label^="Go up"]')!.hasAttribute('disabled')).toBe(false);
  });

  it('drills through an outer ring in one click', async () => {
    const r = await scanned();
    const users = idOf(r, 'Users');
    const alex = r.store.rings()!.children.find((c) => c.node.id === users)!.children.find((c) => c.node.name === 'alex')!.node;
    click(arc(r.el, `n${alex.id}`));
    await settle(r.f);
    expect([...r.el.querySelectorAll('.crumbs li')].map((l) => l.textContent?.replace(/[›\s]+$/, '').trim())).toEqual(['Macintosh HD', 'Users', 'alex']);
  });

  it('marks the segment of a hovered row and the row of a hovered segment', async () => {
    const r = await scanned();
    const users = `n${idOf(r, 'Users')}`;
    const row = [...r.el.querySelectorAll<HTMLElement>('.rows .item')].find((i) => i.textContent?.includes('Users'))!;
    row.dispatchEvent(new MouseEvent('mouseenter'));
    await settle(r.f);
    expect(arc(r.el, users).classList.contains('pd-arc--active')).toBe(true);
    row.dispatchEvent(new MouseEvent('mouseleave'));
    await settle(r.f);
    expect(arc(r.el, users).classList.contains('pd-arc--active')).toBe(false);
    arc(r.el, users).dispatchEvent(new MouseEvent('mouseenter'));
    await settle(r.f);
    expect(row.classList.contains('linked')).toBe(true);
  });

  it('shows an unreadable folder as Unavailable with its reason, never as a number', async () => {
    const r = await scanned();
    const real = r.mock.diskChildren.bind(r.mock);
    vi.spyOn(r.mock, 'diskChildren').mockImplementation(async (q) => {
      const page = await real(q);
      const [first, ...rest] = page.children;
      return first ? { ...page, children: [{ ...first, kind: 'unreadable' as const, unreadableReason: 'needsFullDiskAccess' as const, bytes: 0, items: 0, complete: false, hasChildren: false }, ...rest] } : page;
    });
    await r.store.refresh();
    await settle(r.f);
    const row = r.el.querySelector<HTMLElement>('.rows .item')!;
    expect(row.textContent).toContain('Unavailable');
    expect(row.textContent).toContain('Needs Full Disk Access');
    expect(row.textContent).not.toContain('Unreadable');
    const why = row.querySelector('.size')!.getAttribute('aria-describedby');
    expect(document.getElementById(why!)?.textContent).toContain('Needs Full Disk Access');
    expect(row.textContent).not.toMatch(/\b0 B\b/);
    btn(r.el, 'Table')!.click();
    await settle(r.f);
    const cells = [...r.el.querySelectorAll('tbody tr')].find((tr) => tr.textContent?.includes('Needs Full Disk Access'))!.textContent;
    expect(cells).toContain('Unavailable');
    expect(cells).not.toMatch(/\b0 B\b/);
  });

  it('shows a mount point with its own icon and a dash, never "0 B"', async () => {
    const r = await scanned();
    btn(r.el, 'Table')!.click();
    await settle(r.f);
    const tr = [...r.el.querySelectorAll('tbody tr')].find((t) => t.textContent?.includes('Volumes'))!;
    const mount = r.store.rows().find((x) => x.node.name === 'Volumes')!.node;
    expect(mount.kind).toBe('mountPoint');
    expect((r.f.componentInstance as unknown as { iconOf(n: unknown): string }).iconOf(mount)).toBe('disk');
    expect(tr.textContent).not.toMatch(/\b0 B\b/);
    expect(tr.querySelector('td.num')!.textContent?.trim()).toBe('–');
  });

  it('adds the files that have no row of their own as the last row of the table, so the rows add up', async () => {
    const r = await scanned();
    // The mock's root has no loose files; the user's folder has some.
    for (const name of ['Users', 'alex']) {
      [...r.el.querySelectorAll<HTMLButtonElement>('.rows .name.link')].find((b) => b.textContent?.includes(name))!.click();
      await settle(r.f);
    }
    const loose = r.store.level()!.loose;
    expect(loose.bytes).toBeGreaterThan(0);
    btn(r.el, 'Table')!.click();
    await settle(r.f);
    const foot = r.el.querySelector('tfoot tr, tr.mat-mdc-footer-row')!;
    expect(foot.textContent).toContain('Files directly in this folder');
    expect(foot.textContent).toContain(formatStorage(loose.bytes));
    const input = r.el.querySelector<HTMLInputElement>('input[type="search"]')!;
    input.value = 'ale';
    input.dispatchEvent(new Event('input'));
    await settle(r.f);
    expect(r.el.querySelector('tr.mat-mdc-footer-row')!.classList.contains('foot-off')).toBe(true);
  });

  it('gives focus back to the segment the chart menu was opened for when the menu closes', async () => {
    const r = await scanned();
    const seg = arc(r.el, `n${idOf(r, 'Users')}`);
    seg.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 40 }));
    await settle(r.f);
    document.querySelector<HTMLElement>('.cdk-overlay-backdrop')!.click();
    await settle(r.f, true);
    expect(document.activeElement).toBe(seg);
  });

  it('opens a list row\'s menu from the keyboard (Shift+F10) as a Material menu', async () => {
    const r = await scanned();
    const row = r.el.querySelector<HTMLElement>('.rows .item')!;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle(r.f);
    const menu = document.querySelector('.mat-mdc-menu-panel [aria-label="Folder actions"], [aria-label="Folder actions"]');
    expect(menu).not.toBeNull();
    expect(document.querySelectorAll('.mat-mdc-menu-item').length).toBeGreaterThanOrEqual(3);
  });

  it('opens the table on just the small items when a folded segment is clicked', async () => {
    const r = await scanned();
    const root = r.store.level()!;
    const small = root.children.slice(-2).map((c) => c.id);
    await (r.f.componentInstance as unknown as { onOther(e: { parent: string; ids: string[] }): Promise<void> }).onOther({ parent: `n${root.node.id}`, ids: small.map((i) => `n${i}`) });
    await settle(r.f);
    expect(r.store.view()).toBe('table');
    expect(r.el.querySelectorAll('tbody tr')).toHaveLength(2);
    expect(text(r.el, '.only')).toContain('Show all');
    btn(r.el, 'Show all')!.click();
    await settle(r.f);
    expect(r.el.querySelectorAll('tbody tr').length).toBeGreaterThan(2);
    await (r.f.componentInstance as unknown as { onOther(e: { parent: string; ids: string[] }): Promise<void> }).onOther({ parent: `n${root.node.id}`, ids: [`n${root.node.id}-other`] });
    await settle(r.f);
    expect(r.el.querySelector('.only')).toBeNull();
  });

  it('opens the row menu from the chart for the segment under the pointer', async () => {
    const r = await scanned();
    const users = idOf(r, 'Users');
    arc(r.el, `n${users}`).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 40 }));
    await settle(r.f);
    const menu = document.querySelector('[aria-label="Folder actions"]')!;
    expect([...menu.querySelectorAll('button')].map((b) => b.textContent?.trim())).toEqual(['Open', 'Show in folder', 'Copy path', 'Copy size']);
    expect(document.querySelectorAll('[aria-label="Folder actions"]')).toHaveLength(1);
  });

  it('says when the rings could not be read, keeps the list, and retries from the chart', async () => {
    const r = render();
    await settle(r.f);
    const rings = vi.spyOn(r.mock, 'diskRings').mockRejectedValue({ code: 'internal', message: 'Out of memory.' });
    btn(r.el, 'Scan Macintosh HD')!.click();
    await settle(r.f);
    await settle(r.f, true);
    const chart = r.el.querySelector('pd-sunburst')!;
    expect(chart.getAttribute('data-state')).toBe('error');
    expect(chart.textContent).toContain("Couldn't draw the rings: Out of memory.");
    expect(names(r.el)[0]).toBe('Users');
    rings.mockRestore();
    [...chart.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Retry'))!.click();
    await settle(r.f);
    expect(r.el.querySelector('pd-sunburst')!.getAttribute('data-state')).toBe('ready');
    expect(r.el.querySelector('pd-sunburst path[role="treeitem"]')).not.toBeNull();
  });

  it('shows the reason when a scan is refused for lack of room, and Rescan works again once it can', async () => {
    const r = await scanned();
    const reason = 'There is not room for another scan next to the running ones. Finish or close a scan first.';
    const start = vi.spyOn(r.mock, 'startDiskScan').mockRejectedValueOnce({ code: 'busy', message: reason });
    btn(r.el, 'Rescan Macintosh HD')!.click();
    await settle(r.f);
    const notice = r.el.querySelector('.callout.bad[role="alert"]')!;
    expect(notice.textContent).toContain(reason);
    expect(r.store.outcome()).toBe('done');
    expect(r.el.querySelector('.rows')).not.toBeNull();
    const rescan = btn(r.el, 'Rescan Macintosh HD')!;
    expect(rescan.hasAttribute('disabled')).toBe(false);
    rescan.click();
    await settle(r.f);
    expect(start).toHaveBeenCalledTimes(2);
    expect(r.el.querySelector('.job')).not.toBeNull();
    expect(r.el.querySelector('.callout.bad[role="alert"]')).toBeNull();
  });

  describe('saved maps (ADR 0029)', () => {
    const DAY = 86_400_000;
    const opts = { savedMaps: [{ target: { volume: { id: '/' } }, ageMs: 3 * DAY, changedBytes: 12_000_000_000 }] };

    async function opened() {
      const r = render(opts);
      await settle(r.f);
      await settle(r.f);
      return r;
    }

    it('shows the saved map at once with "saved", its age and the net change in used space', async () => {
      const { f, el } = await opened();
      expect(f).toBeTruthy();
      expect(text(el, '.page-head p')).toContain('scanned 3 days ago (saved)');
      const note = text(el, '[data-testid="saved-map"]')!;
      expect(note).toContain('Saved map, scanned 3 days ago.');
      expect(note).toContain(`Net change in used space since: +${formatStorage(12_000_000_000)}`);
      expect(note).toContain('a net figure');
      expect(names(el)).toContain('Users');
      expect(btn(el, 'Forget this map')).toBeDefined();
      expect(el.querySelector('.job')).toBeNull();
    });

    it('keeps the saved map during a Rescan, with progress, Show progress and Cancel, and swaps when it is done', async () => {
      const { f, el, store } = await opened();
      const old = store.status()!.scan;
      btn(el, 'Rescan Macintosh HD')!.click();
      await settle(f);
      expect(store.status()!.scan).toBe(old);
      expect(text(el, '[aria-label="Rescan progress"] .job-title')).toContain('the map below stays until this is done');
      expect(btn(el, 'Show progress')).toBeDefined();
      expect(btn(el, 'Cancel rescan')).toBeDefined();
      expect((btn(el, 'Rescan Macintosh HD') as HTMLButtonElement).disabled).toBe(true);
      expect(names(el)).toContain('Users');
      await settle(f, true);
      expect(store.status()!.scan).not.toBe(old);
      expect(el.querySelector('[aria-label="Rescan progress"]')).toBeNull();
      expect(el.querySelector('[data-testid="saved-map"]')).toBeNull();
      expect(text(el, '.page-head p')).not.toContain('(saved)');
    });

    it('keeps the saved map when the Rescan is cancelled', async () => {
      const { f, el, store } = await opened();
      const old = store.status()!.scan;
      btn(el, 'Rescan Macintosh HD')!.click();
      await settle(f);
      btn(el, 'Cancel rescan')!.click();
      await settle(f);
      expect(store.status()!.scan).toBe(old);
      expect(el.querySelector('[aria-label="Rescan progress"]')).toBeNull();
      expect(el.querySelector('[data-testid="saved-map"]')).not.toBeNull();
      expect(names(el)).toContain('Users');
    });

    it('shows the growing tree on request', async () => {
      const { f, el, store } = await opened();
      const old = store.status()!.scan;
      btn(el, 'Rescan Macintosh HD')!.click();
      await settle(f);
      btn(el, 'Show progress')!.click();
      await settle(f);
      expect(store.status()!.scan).not.toBe(old);
      expect(el.querySelector('[aria-label="Scan progress"]')).not.toBeNull();
    });

    it('forgets the saved map and shows the empty state', async () => {
      const { f, el, mock } = await opened();
      btn(el, 'Forget this map')!.click();
      await settle(f);
      expect(el.querySelector('[data-testid="saved-map"]')).toBeNull();
      expect(el.querySelector('[aria-label="Not scanned"]')).not.toBeNull();
      expect(await mock.diskScans()).toEqual([]);
    });

    it('says why a finished scan was not kept', async () => {
      const { f, el, store } = await opened();
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, notSaved: 'tooBig' });
      await settle(f);
      expect(text(el, '[data-testid="not-saved"]')).toBe('Too big to keep; raise the limit in Settings.');
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, notSaved: 'noSpace' });
      await settle(f);
      expect(text(el, '[data-testid="not-saved"]')).toContain('too little free space');
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, notSaved: 'failed' });
      await settle(f);
      expect(text(el, '[data-testid="not-saved"]')).toContain('The earlier saved map is unchanged');
    });

    it('says no change when the used space is the same, and omits the figure for a folder', async () => {
      const { f, el, store } = await opened();
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, changedBytes: 0 });
      await settle(f);
      expect(text(el, '[data-testid="saved-map"]')).toContain('Used space is the same');
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, changedBytes: null });
      await settle(f);
      const note = text(el, '[data-testid="saved-map"]')!;
      expect(note).not.toContain('Net change');
      store.receive({ ...store.status()!, seq: store.status()!.seq + 1, changedBytes: -2_000_000_000 });
      await settle(f);
      expect(text(el, '[data-testid="saved-map"]')).toContain(`\u2212${formatStorage(2_000_000_000)}`);
    });

    it('says the Rescan stopped and keeps the map', async () => {
      const r = render({ savedMaps: [{ target: { volume: { id: '/Volumes/Media' } }, ageMs: DAY, changedBytes: null }] });
      await settle(r.f);
      await r.store.selectVolume('/Volumes/Media');
      await settle(r.f);
      btn(r.el, 'Rescan Media')!.click();
      await settle(r.f, true);
      expect(text(r.el, '.callout.bad')).toContain('The rescan stopped.');
      expect(r.el.querySelector('[data-testid="saved-map"]')).not.toBeNull();
    });
  });
});
