import { TestBed } from '@angular/core/testing';
import { WatchesPage } from './watches-page';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, byText, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  const f = TestBed.createComponent(WatchesPage);
  TestBed.inject(WatchesStore).start();
  await settle(f);
  return { f, backend, store: TestBed.inject(WatchesStore) };
}
const rows = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll('tbody tr')) as HTMLElement[];
const names = (f: { nativeElement: HTMLElement }) => rows(f).map((r) => text(r.querySelector('.name')));

describe('WatchesPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('lists every watch sorted by name with counts, status words and next-check', async () => {
    const { f } = await make();
    expect(names(f)).toEqual([
      'Acme Installer', 'Fjord UI kit', 'Lumen CLI', 'Nightly docs feed', 'Orbit API image', 'Orbit server', 'tinyparse', 'Vendor download page',
    ]);
    expect(text(f.nativeElement.querySelector('.sub'))).toMatch(/^8 watches · \d+ with new releases$/);
    const docs = rows(f)[3]!;
    expect(text(docs)).toContain('Paused');
    expect(text(rows(f)[5]!)).toContain('Unreachable');
    expect(text(rows(f)[5]!)).toContain('gitlab.example did not answer');
  });

  it('searches, filters and says when nothing matches, with a way back', async () => {
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
    const radios = Array.from(f.nativeElement.querySelectorAll('pd-segmented-control [role=radio], pd-segmented-control button')) as HTMLElement[];
    (radios.find((b) => text(b) === 'Paused') as HTMLElement).click();
    await settle(f);
    expect(names(f)).toEqual(['Nightly docs feed']);
  });

  it('sorts', async () => {
    const { f } = await make();
    const select = f.nativeElement.querySelector('select[aria-label="Sort watches"]') as HTMLSelectElement;
    typeInto(select, 'checked', 'change');
    await settle(f);
    expect(names(f)[0]).not.toBe('Acme Installer');
  });

  it('runs the quick actions through the backend', async () => {
    const { f, backend } = await make();
    (f.nativeElement.querySelector('button[aria-label="Check Acme Installer now"]') as HTMLButtonElement).click();
    (f.nativeElement.querySelector('button[aria-label="Pause Acme Installer"]') as HTMLButtonElement).click();
    (f.nativeElement.querySelector('button[aria-label="Mark Acme Installer as seen"]') as HTMLButtonElement).click();
    (f.nativeElement.querySelector('button[aria-label="Open the latest release of Acme Installer"]') as HTMLButtonElement).click();
    (byText(f.nativeElement, 'button', 'Check all now') as HTMLButtonElement).click();
    (byText(f.nativeElement, 'button', 'Mark all seen') as HTMLButtonElement).click();
    await settle(f);
    expect(backend.calls).toEqual(expect.arrayContaining(['checkNow', 'setPaused', 'acknowledge', 'openRelease']));
    expect(f.nativeElement.querySelector('button[aria-label="Resume Acme Installer"]')).not.toBeNull();
    expect(byText(f.nativeElement, 'button', 'Mark all seen')).toBeNull();
  });

  it('invites the first watch when there are none', async () => {
    const { f } = await make(makeMock({ scenario: 'empty' }));
    expect(text(f.nativeElement)).toContain('No watches yet');
    expect(text(f.nativeElement.querySelector('.sub'))).toBe('0 watches');
    expect((byText(f.nativeElement, 'button', 'Check all now') as HTMLButtonElement).disabled).toBe(true);
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
