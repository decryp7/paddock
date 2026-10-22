import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WatchesPage } from './watches-page';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend, [{ path: 'watches/:id', children: [] }]) });
  const f = TestBed.createComponent(WatchesPage);
  TestBed.inject(WatchesStore).start();
  await settle(f);
  return { f, backend, store: TestBed.inject(WatchesStore) };
}
const rows = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll('tbody tr')) as HTMLElement[];
const names = (f: { nativeElement: HTMLElement }) => rows(f).map((r) => text(r.querySelector('.name')));

describe('WatchesPage', () => {
  afterEach(() => vi.restoreAllMocks());

  const menu = () => Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
  const openMenu = async (f: { nativeElement: HTMLElement; detectChanges(): void }, label: string) => {
    (f.nativeElement.querySelector(`button[aria-label="${label}"]`) as HTMLButtonElement).click();
    f.detectChanges();
    await flush(10);
    return menu();
  };
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = '')));

  it('lists every watch sorted by name with counts, status and next-check', async () => {
    const { f } = await make();
    expect(names(f)).toEqual([
      'Acme Installer', 'Fjord UI kit', 'Lumen CLI', 'Nightly docs feed', 'Orbit API image', 'Orbit server', 'tinyparse', 'Vendor download page',
    ]);
    expect(text(f.nativeElement.querySelector('.sub'))).toMatch(/^8 watches · \d+ with new releases$/);
    expect(text(rows(f)[3]!)).toContain('Paused');
    expect(text(rows(f)[5]!)).toContain('Unreachable');
    expect(rows(f)[0]!.querySelector('sp-avatar')).not.toBeNull();
    expect(text(rows(f)[0]!.querySelector('.src'))).toContain('·');
  });

  it('marks unseen watches with a dot and shows installed to latest only when newer', async () => {
    const { f, store } = await make();
    const dots = rows(f).map((r) => !r.querySelector('.unseen-dot')!.classList.contains('off'));
    expect(dots.filter(Boolean)).toHaveLength(store.unseen().length);
    const upgradeRows = rows(f).filter((r) => r.querySelector('.token'));
    expect(upgradeRows).toHaveLength(store.watches().filter((w) => w.newerThanInstalled && w.installed && w.latest).length);
  });

  it('searches, filters with the tabs and says when nothing matches, with a way back', async () => {
    const { f, store } = await make();
    typeInto(f.nativeElement.querySelector('input[type=search]'), 'orbit');
    await settle(f);
    expect(names(f)).toEqual(['Orbit API image', 'Orbit server']);
    typeInto(f.nativeElement.querySelector('input[type=search]'), 'zzzz');
    await settle(f);
    expect(text(f.nativeElement)).toContain('No watches match');
    (byText(f.nativeElement, 'button', 'Clear search and filter') as HTMLButtonElement).click();
    await settle(f);
    expect(store.query()).toBe('');
    expect(rows(f)).toHaveLength(8);
    const tabs = () => Array.from(f.nativeElement.querySelectorAll('.seg-tabs button')) as HTMLElement[];
    expect(tabs().map((b) => text(b))).toEqual(['All', 'New', 'Needs attention', 'Paused']);
    expect(tabs()[0]!.getAttribute('aria-pressed')).toBe('true');
    tabs().find((b) => text(b) === 'Paused')!.click();
    await settle(f);
    expect(names(f)).toEqual(['Nightly docs feed']);
    expect(tabs().find((b) => text(b) === 'Paused')!.getAttribute('aria-pressed')).toBe('true');
    tabs().find((b) => text(b) === 'Needs attention')!.click();
    await settle(f);
    expect(names(f)).toEqual(store.attention().map((w) => w.name).sort((a, b) => a.localeCompare(b)));
    expect(names(f)).toContain('Orbit server');
  });

  it('sorts from the column header buttons and marks the sorted column', async () => {
    const { f, store } = await make();
    const th = (label: string) => byText(f.nativeElement, 'th', label)!;
    expect(th('Watch').getAttribute('aria-sort')).toBe('ascending');
    expect(th('Checked').getAttribute('aria-sort')).toBeNull();
    (th('Checked').querySelector('button') as HTMLButtonElement).click();
    await settle(f);
    expect(store.sort()).toBe('checked');
    expect(names(f)[0]).not.toBe('Acme Installer');
    expect(th('Checked').getAttribute('aria-sort')).not.toBeNull();
    expect(th('Watch').getAttribute('aria-sort')).toBeNull();
    (th('Latest').querySelector('button') as HTMLButtonElement).click();
    await settle(f);
    expect(store.sort()).toBe('latest');
    expect(th('Latest').querySelector('.arrow')!.classList.contains('on')).toBe(true);
  });

  it('runs the hover actions and the row menu through the backend', async () => {
    const { f, backend } = await make();
    (f.nativeElement.querySelector('button[aria-label="Check Acme Installer now"]') as HTMLButtonElement).click();
    (f.nativeElement.querySelector('button[aria-label="Open the latest release of Acme Installer"]') as HTMLButtonElement).click();
    await settle(f);
    let items = await openMenu(f, 'More actions for Acme Installer');
    expect(items.map((i) => text(i))).toEqual(['Open details', 'Mark as seen', 'Pause']);
    items[1]!.click();
    await settle(f);
    expect(rows(f)[0]!.querySelector('.unseen-dot')!.classList.contains('off')).toBe(true);
    items = await openMenu(f, 'More actions for Acme Installer');
    expect(items.map((i) => text(i))).toEqual(['Open details', 'Pause']);
    items[1]!.click();
    await settle(f);
    items = await openMenu(f, 'More actions for Acme Installer');
    expect(items.map((i) => text(i))).toContain('Resume');
    expect(backend.calls).toEqual(expect.arrayContaining(['checkNow', 'setPaused', 'acknowledge', 'openRelease']));
  });

  it('opens the details from the row menu, a row click and the name link without double navigation', async () => {
    const { f } = await make();
    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    await openMenu(f, 'More actions for Fjord UI kit');
    byText(document.body, '[cdkMenuItem]', 'Open details')!.click();
    expect(nav).toHaveBeenLastCalledWith(['/watches', expect.any(String)]);
    rows(f)[0]!.click();
    expect(nav).toHaveBeenCalledTimes(2);
    (rows(f)[0]!.querySelector('a.name') as HTMLAnchorElement).addEventListener('click', (e) => e.preventDefault());
    (rows(f)[0]!.querySelector('a.name') as HTMLAnchorElement).click();
    expect(nav).toHaveBeenCalledTimes(2);
  });

  it('runs Check all now and Mark all seen from the header menu, which disables what does not apply', async () => {
    const { f, backend } = await make();
    let items = await openMenu(f, 'Watches actions');
    expect(items.map((i) => text(i))).toEqual(['Check all now', 'Mark all seen']);
    items[0]!.click();
    await settle(f);
    await openMenu(f, 'Watches actions');
    byText(document.body, '[cdkMenuItem]', 'Mark all seen')!.click();
    await settle(f);
    expect(backend.calls).toEqual(expect.arrayContaining(['checkNow', 'acknowledge']));
    expect(text(f.nativeElement.querySelector('.sub'))).toBe('8 watches');
    document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = ''));
    items = await openMenu(f, 'Watches actions');
    expect(items[1]!.getAttribute('aria-disabled')).toBe('true');
  });

  it('invites the first watch when there are none', async () => {
    const { f } = await make(makeMock({ scenario: 'empty' }));
    expect(text(f.nativeElement)).toContain('No watches yet');
    expect(text(f.nativeElement.querySelector('.sub'))).toBe('0 watches');
    const items = await openMenu(f, 'Watches actions');
    expect(items.map((i) => i.getAttribute('aria-disabled'))).toEqual(['true', 'true']);
    expect(byText(f.nativeElement, 'a', 'Add your first watch')).not.toBeNull();
  });

  it('shows a failed load with Retry instead of an empty page', async () => {
    const { f } = await make(makeMock({ scenario: 'broken' }));
    expect(text(f.nativeElement)).toContain('Could not load the watch list');
    expect(text(f.nativeElement)).toContain('The engine is not answering');
    (byText(f.nativeElement, 'button', /retry/i) as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Could not load the watch list');
  });
});
