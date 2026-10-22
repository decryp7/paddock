import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WhatsNew } from './whats-new';
import type { WhatsNewItem } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { Clipboard } from '../../shared/clipboard';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, byText, flush, makeMock, NOW, settle, text } from '../../../testing/test-helpers';

const item = (over: Partial<WhatsNewItem> = {}, release: Partial<WhatsNewItem['release']> = {}): WhatsNewItem => ({
  watchId: 'w-1',
  watchName: 'Acme Installer',
  sourceKind: 'folder',
  release: { version: '4.0.0-rc.1', prerelease: true, name: 'n', location: 'l', publishedAtMs: null, firstSeenMs: 1, size: null, sha256: null, product: null, channel: '', fields: {}, artifacts: [], updatedInPlace: false, ...release },
  releasedAtMs: NOW - 3_600_000,
  newerThanInstalled: false,
  unseen: false,
  ...over,
});

describe('WhatsNew', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = '')));

  async function make(items: WhatsNewItem[], backend = makeMock()) {
    TestBed.configureTestingModule({ providers: appProviders(backend, [{ path: 'watches/:id', children: [] }]) });
    TestBed.inject(ClockService).set(NOW);
    TestBed.inject(WatchesStore).start();
    const f = TestBed.createComponent(WhatsNew);
    f.componentRef.setInput('items', items);
    await settle(f);
    return { f, backend };
  }
  const rows = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll<HTMLElement>('li.item'));

  it('flags pre-releases, omits an unknown size and shows a seen row quietly', async () => {
    const { f } = await make([item()]);
    const t = text(f.nativeElement);
    expect(t).toContain('4.0.0-rc.1');
    expect(t).toContain('pre-release');
    expect(t).not.toMatch(/MiB|KiB/);
    const row = rows(f)[0]!;
    expect(row.classList.contains('seen')).toBe(true);
    expect(row.querySelector('.unseen-dot')!.classList.contains('off')).toBe(true);
    expect(text(row.querySelector('.when'))).toBe('1 h');
  });

  it('shows an empty state', async () => {
    const { f } = await make([]);
    expect(text(f.nativeElement)).toContain('No releases yet');
    expect(rows(f)).toHaveLength(0);
  });

  it('groups by day with the product, size and file count in the meta line', async () => {
    const { f } = await make([
      item({ unseen: true }, { prerelease: false, product: 'Server', size: 5 * 1048576, artifacts: [{ name: 'a' }, { name: 'b' }] as never }),
      item({ releasedAtMs: NOW - 26 * 3_600_000 }, { prerelease: false, name: 'Plain name' }),
    ]);
    expect(Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('h3')).map((h) => text(h)).slice(0, 2)).toEqual(['Today', 'Yesterday']);
    const [a, b] = rows(f);
    expect(text(a!.querySelector('.meta'))).toBe('Server · 5.0 MiB · 2 files');
    expect(a!.querySelector('.unseen-dot')!.classList.contains('off')).toBe(false);
    expect(a!.querySelector('.pre')).toBeNull();
    expect(text(b!.querySelector('.meta'))).toBe('Plain name');
  });

  it('shows the installed to latest token only when the release is newer than the installed version', async () => {
    const backend = makeMock();
    const { f } = await make([], backend);
    const installed = TestBed.inject(WatchesStore).byId('w-1')!.installed;
    expect(installed).not.toBeNull();
    f.componentRef.setInput('items', [item({ newerThanInstalled: true }, { version: '9.9.9' }), item({ newerThanInstalled: false }, { version: '9.9.8' })]);
    await settle(f);
    const [newer, same] = rows(f);
    expect(newer!.querySelector('.token mat-icon')).not.toBeNull();
    expect(text(newer!.querySelector('.token'))).toContain(installed!);
    expect(same!.querySelector('.token mat-icon')).toBeNull();
  });

  it('shows no installed version for a watch that has none', async () => {
    const { f } = await make([item({ watchId: 'gone', newerThanInstalled: true }, { version: '1.2.3' })]);
    expect(rows(f)[0]!.querySelector('.token mat-icon')).toBeNull();
  });

  it('opens the watch on click and Enter, and marks seen with E only while unseen', async () => {
    const { f, backend } = await make([item({ unseen: true }, { version: '9.9.9' }), item({ watchId: 'w-2' }, { version: '1.0.0' })]);
    const router = TestBed.inject(Router);
    const nav = vi.spyOn(router, 'navigate').mockResolvedValue(true);
    const [a, b] = rows(f);
    a!.click();
    b!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(nav.mock.calls.map((c) => c[0])).toEqual([['/watches', 'w-1'], ['/watches', 'w-2']]);
    b!.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));
    await flush(2);
    expect(backend.calls).not.toContain('acknowledge');
    a!.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', bubbles: true }));
    await flush(2);
    expect(backend.calls).toContain('acknowledge');
  });

  it('keeps hover action clicks from opening the watch', async () => {
    const { f, backend } = await make([item({}, { version: '9.9.9' })]);
    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    (rows(f)[0]!.querySelector('button[aria-label^="Open "]') as HTMLElement).click();
    (rows(f)[0]!.querySelector('button[aria-label^="Download "]') as HTMLElement).click();
    await flush(5);
    expect(nav).not.toHaveBeenCalled();
    expect(backend.calls).toEqual(expect.arrayContaining(['openRelease', 'downloadRelease']));
  });

  async function openMenu(f: { nativeElement: HTMLElement; detectChanges(): void }) {
    (f.nativeElement.querySelector('sp-menu-button button') as HTMLElement).click();
    f.detectChanges();
    await flush(10);
    return Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
  }

  it('offers Mark as seen, Copy link and Copy SHA-256 in the menu according to the release', async () => {
    const { f, backend } = await make([item({ unseen: true }, { location: 'https://x/y', sha256: 'abc123', version: '9.9.9' })]);
    const copy = vi.spyOn(TestBed.inject(Clipboard), 'copy').mockResolvedValue(true as never);
    const entries = await openMenu(f);
    expect(entries.map((e) => text(e))).toEqual(['Mark as seen', 'Copy link', 'Copy SHA-256']);
    byText(document.body, '[cdkMenuItem]', 'Copy link')!.click();
    expect(copy).toHaveBeenCalledWith('https://x/y', 'Link');
    await openMenu(f);
    byText(document.body, '[cdkMenuItem]', 'Copy SHA-256')!.click();
    expect(copy).toHaveBeenCalledWith('abc123', 'Checksum');
    await openMenu(f);
    byText(document.body, '[cdkMenuItem]', 'Mark as seen')!.click();
    await flush(5);
    expect(backend.calls).toContain('acknowledge');
  });

  it('leaves out Mark as seen and the checksum when they do not apply', async () => {
    const { f } = await make([item()]);
    const entries = await openMenu(f);
    expect(entries.map((e) => text(e))).toEqual(['Copy link']);
  });
});
