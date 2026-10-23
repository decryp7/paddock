import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WatchDetailPage } from './watch-detail-page';
import { Confirm } from '../../shared/ui/confirm-dialog';
import { ToastStore } from '../../state/toast.store';
import { WatchesStore } from '../../state/watches.store';
import type { WatchSummary } from '../../core/api.types';
import { appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';

@Component({ template: '' })
class Stub {}
const routes = [{ path: 'watches', component: Stub }];

async function make(id = 'w-1', backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
  TestBed.inject(WatchesStore).start();
  const f = TestBed.createComponent(WatchDetailPage);
  f.componentRef.setInput('id', id);
  await settle(f);
  return { f, backend };
}
type F = Awaited<ReturnType<typeof make>>['f'];
const click = async (f: F, el: Element | null) => {
  (el as HTMLElement).click();
  await settle(f);
};
const button = (f: F, label: string | RegExp) => byText(f.nativeElement, 'button', label) as HTMLButtonElement;
const q = (f: F, sel: string) => f.nativeElement.querySelector(sel) as HTMLElement;

describe('WatchDetailPage', () => {
  afterEach(() => vi.restoreAllMocks());

  const menu = () => Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
  const openMenu = async (f: F, label: string) => {
    (q(f, `button[aria-label="${label}"]`) as HTMLButtonElement).click();
    f.detectChanges();
    await flush(10);
    return menu();
  };
  const pick = async (f: F, label: string, item: string) => {
    await openMenu(f, label);
    (byText(document.body, '.cdk-overlay-container [cdkMenuItem]', item) as HTMLElement).click();
    await settle(f);
  };
  const releaseRows = (f: F) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('section[aria-labelledby=hist-h] tbody tr'));
  const productRows = (f: F) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('section[aria-labelledby=prod-h] tbody tr'));
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((e) => (e.innerHTML = '')));

  it('shows the header, the hero with latest and installed, the upgrade line, history, checks and settings', async () => {
    const { f } = await make();
    expect(text(q(f, 'h1'))).toBe('Acme Installer');
    expect(q(f, '.head sp-avatar')).not.toBeNull();
    expect(text(q(f, '.head .src'))).toContain('Folder or share');
    expect(text(q(f, '.head sp-status-dot'))).toBe('Working');
    const stats = Array.from(f.nativeElement.querySelectorAll('.stat') as NodeListOf<HTMLElement>);
    expect(text(stats[0]).startsWith('Latest')).toBe(true);
    expect(text(stats[0]!.querySelector('sp-ver'))).toMatch(/^R\d+\.\d+\.\d+T\d+$/);
    expect(text(stats[1]).startsWith('Installed')).toBe(true);
    expect(q(f, '.upgrade .token')).not.toBeNull();
    expect(text(q(f, '.upgrade'))).toContain('is available');
    expect(button(f, 'Mark as seen')).not.toBeNull();
    expect(text(q(f, 'dl.facts'))).toMatch(/Found.*Published.*Checked.*Next/);
    expect(releaseRows(f)).toHaveLength(36);
    expect(text(q(f, '#hist-h'))).toBe('Releases');
    expect(text(q(f, 'section[aria-labelledby=hist-h] .help'))).toBe('36 found');
    expect(byText(f.nativeElement, 'th', 'Size (MiB)')).not.toBeNull();
    expect(text(releaseRows(f)[0]!.querySelector('td.num'))).toMatch(/^\d+(\.\d+)?$/);
    expect(releaseRows(f).filter((r) => r.querySelector('.installed'))).toHaveLength(1);
    const settings = text(q(f, 'dl.kv'));
    expect(settings).toContain('ChecksEvery 6 hours');
    expect(settings).toContain('VersionsBuild number');
    expect(settings).toContain('NotifyEvery new release');
    expect(settings).toContain('Open the release location');
    expect(f.nativeElement.querySelector('section[aria-labelledby=prod-h]')).toBeNull();
  });

  it('marks failed and changed checks in the strip, with a title for each and the last failure', async () => {
    const { f, backend } = await make();
    const cells = Array.from(f.nativeElement.querySelectorAll('.strip .cell') as NodeListOf<HTMLElement>);
    const checks = (await backend.getWatch('w-1')).checks;
    expect(cells).toHaveLength(30);
    expect(q(f, '.strip').getAttribute('aria-label')).toBe('The last 30 checks, oldest first');
    expect(cells.filter((c) => c.classList.contains('bad')).length).toBe(checks.filter((c) => c.error).length);
    expect(cells.filter((c) => c.classList.contains('changed')).length).toBeGreaterThan(0);
    expect(cells.every((c) => !c.classList.contains('bad') || !c.classList.contains('changed'))).toBe(true);
    const bad = cells.find((c) => c.classList.contains('bad'));
    if (bad) expect(bad.getAttribute('title')).toContain('failed, ');
    expect(cells.find((c) => !c.classList.contains('bad'))!.getAttribute('title')).toContain('found ');
    if (bad) expect(text(q(f, 'p.bad'))).toBe('Last failure: The server did not answer within 20 seconds.');
  });

  it('says a watch was not checked yet when it has no checks', async () => {
    const backend = makeMock({ scenario: 'products' });
    const { f } = await make('w-9', backend);
    expect(f.nativeElement.querySelectorAll('.strip .cell')).toHaveLength(1);
    expect(q(f, 'p.bad')).toBeNull();
    const got = await backend.getWatch('w-9');
    expect(got.checks).toHaveLength(1);
  });

  it('shows the problem of a failing watch and no next check for a paused one', async () => {
    const orbit = await make('w-4');
    expect(text(q(orbit.f, '.problem'))).toContain('gitlab.example did not answer');
    expect(text(q(orbit.f, '.head sp-status-dot'))).toBe('Unreachable');
    TestBed.resetTestingModule();
    const docs = await make('w-8');
    expect(text(q(docs.f, 'dl.facts'))).not.toContain('Next');
    expect(q(docs.f, 'button[aria-label="Resume"]')).not.toBeNull();
    expect(text(q(docs.f, '.head sp-status-dot'))).toBe('Paused');
  });

  it('shows "No release yet", "Not set" and an empty history for a watch that found nothing', async () => {
    const backend = makeMock();
    const bare = (w: WatchSummary): WatchSummary => ({ ...w, latest: null, installed: null, unseen: false, newerThanInstalled: false });
    const realGet = backend.getWatch.bind(backend);
    const realList = backend.listWatches.bind(backend);
    vi.spyOn(backend, 'getWatch').mockImplementation(async (id) => {
      const d = await realGet(id);
      return { ...d, summary: bare(d.summary), history: [], checks: [] };
    });
    vi.spyOn(backend, 'listWatches').mockImplementation(async () => (await realList()).map(bare));
    const { f } = await make('w-2', backend);
    expect(text(q(f, 'h1'))).toBe('Lumen CLI');
    expect(text(q(f, 'section[aria-labelledby=checks-h]'))).toContain('Not checked yet.');
    expect(text(q(f, 'dl.facts'))).not.toContain('Published');
    expect(text(q(f, '.hero'))).toContain('No release yet');
    expect(text(q(f, '.hero'))).toContain('Not set');
    expect(text(f.nativeElement)).toContain('No releases found yet');
    expect(q(f, '.upgrade')).toBeNull();
    expect(button(f, 'Mark as seen')).toBeNull();
  });

  it('marks seen, pauses and checks now through the store', async () => {
    const { f, backend } = await make();
    await click(f, button(f, 'Mark as seen'));
    expect(button(f, 'Mark as seen')).toBeNull();
    await click(f, q(f, 'button[aria-label="Pause"]'));
    expect(q(f, 'button[aria-label="Resume"]')).not.toBeNull();
    await click(f, q(f, 'button[aria-label="Check now"]'));
    expect(backend.calls).toEqual(expect.arrayContaining(['acknowledge', 'setPaused', 'checkNow']));
  });

  it('downloads and opens the latest release from the upgrade line', async () => {
    const { f, backend } = await make();
    await click(f, byText(q(f, '.upgrade'), 'button', 'Download'));
    await click(f, byText(q(f, '.upgrade'), 'button', 'Open'));
    expect(backend.calls).toEqual(expect.arrayContaining(['downloadRelease', 'openRelease']));
  });

  it('copies the address from the header button and the menu', async () => {
    const { f } = await make();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const address = text(q(f, '.where'));
    await click(f, q(f, 'button[aria-label="Copy the address"]'));
    expect(writeText).toHaveBeenCalledWith(address);
    writeText.mockClear();
    await pick(f, 'More actions', 'Copy address');
    expect(writeText).toHaveBeenCalledWith(address);
    expect(TestBed.inject(ToastStore).toasts().map((t) => t.title)).toContain('Address copied');
  });

  it('opens the release side panel on click, shows its details and files, and Esc closes it and returns focus', async () => {
    const { f, backend } = await make();
    const rows = releaseRows(f);
    expect(q(f, 'aside.panel')).toBeNull();
    rows[2]!.focus();
    rows[2]!.click();
    await settle(f);
    const panel = q(f, 'aside.panel');
    const version = (await backend.getWatch('w-1')).history[2]!;
    expect(panel.getAttribute('aria-label')).toBe('Release details');
    expect(text(panel.querySelector('h2'))).toBe(version.version);
    expect(text(panel)).toContain('Location');
    expect(releaseRows(f)[2]!.classList.contains('on')).toBe(true);
    expect(q(f, '.body').classList.contains('with-panel')).toBe(true);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(q(f, 'aside.panel')).toBeNull();
    expect(document.activeElement).toBe(releaseRows(f)[2]);
    // Enter on a row opens it too, and the close button works.
    releaseRows(f)[1]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(f);
    expect(q(f, 'aside.panel')).not.toBeNull();
    await click(f, q(f, 'button[aria-label="Close details"]'));
    expect(q(f, 'aside.panel')).toBeNull();
  });

  it('leaves the panel open when Esc closes a menu', async () => {
    const { f } = await make();
    releaseRows(f)[0]!.click();
    await settle(f);
    await openMenu(f, 'More actions');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(q(f, 'aside.panel')).not.toBeNull();
  });

  it('opens and downloads a single file from the panel, and copies the location and checksum', async () => {
    const backend = makeMock({ scenario: 'products' });
    const { f } = await make('w-9', backend);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const rel = (await backend.getWatch('w-9')).history.find((r) => r.artifacts.length === 2)!;
    const row = releaseRows(f).find((r) => text(r).includes(rel.version) && text(r).includes('PRO'))!;
    row.click();
    await settle(f);
    const files = Array.from(f.nativeElement.querySelectorAll('ul.files li') as NodeListOf<HTMLElement>);
    expect(files).toHaveLength(2);
    expect(text(files[1])).toContain('x64');
    expect(text(files[1])).toContain('checksum file');
    await click(f, q(f, `button[aria-label="Open ${rel.artifacts[1]!.name}"]`));
    await click(f, q(f, `button[aria-label="Download ${rel.artifacts[1]!.name}"]`));
    await click(f, q(f, 'button[aria-label="Copy the location"]'));
    expect(writeText).toHaveBeenCalledWith(rel.location);
    expect(q(f, 'button[aria-label="Copy the checksum"]')).toBeNull(); // no checksum for this release
    expect(backend.calls).toEqual(expect.arrayContaining(['openRelease', 'downloadRelease']));
    await click(f, byText(q(f, '.panel-actions'), 'button', 'Open'));
    await click(f, byText(q(f, '.panel-actions'), 'button', 'Download'));
  });

  it('copies the checksum when a release has one', async () => {
    const { f, backend } = await make();
    const withSha = (await backend.getWatch('w-1')).history.findIndex((r) => r.sha256);
    expect(withSha).toBeGreaterThanOrEqual(0);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    releaseRows(f)[withSha]!.click();
    await settle(f);
    await click(f, q(f, 'button[aria-label="Copy the checksum"]'));
    expect(writeText).toHaveBeenCalledWith((await backend.getWatch('w-1')).history[withSha]!.sha256);
  });

  it('shortens a long location in the middle but keeps the full text in the title', async () => {
    const { f, backend } = await make();
    releaseRows(f)[0]!.click();
    await settle(f);
    const loc = (await backend.getWatch('w-1')).history[0]!.location;
    const code = q(f, '.copyable .code');
    expect(code.getAttribute('title')).toBe(loc);
    expect(text(code).length).toBeLessThanOrEqual(38);
  });

  it('runs the per-release actions: open, download, and the menu entries', async () => {
    const { f, backend } = await make();
    const version = (await backend.getWatch('w-1')).history[0]!;
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await click(f, q(f, `button[aria-label="Open ${version.version}"]`));
    await click(f, q(f, `button[aria-label="Download ${version.version}"]`));
    const label = `More actions for ${version.version}`;
    const entries = await openMenu(f, label);
    expect(entries.map((e) => text(e))).toEqual(version.sha256
        ? ['Show details', 'Open release', 'Download', 'Copy link', 'Copy SHA-256', 'Mark as installed', 'Mark as seen']
        : ['Show details', 'Open release', 'Download', 'Copy link', 'Mark as installed', 'Mark as seen']);
    byText(document.body, '[cdkMenuItem]', 'Copy link')!.click();
    expect(writeText).toHaveBeenCalledWith(version.location);
    await pick(f, label, 'Mark as seen');
    await pick(f, label, 'Mark as installed');
    expect(backend.calls).toEqual(expect.arrayContaining(['openRelease', 'downloadRelease', 'acknowledge', 'setInstalled']));
    expect((await backend.getWatch('w-1')).summary.acknowledged).toBe(version.version);
    expect((await backend.getWatch('w-1')).summary.installed).toBe(version.version);
    expect(TestBed.inject(ToastStore).toasts().map((t) => t.title)).toEqual(expect.arrayContaining(['Download finished', 'Link copied']));
    expect(releaseRows(f)[0]!.querySelector('.installed')).not.toBeNull();
  });

  it('copies the SHA-256 from the release menu', async () => {
    const { f, backend } = await make();
    const history = (await backend.getWatch('w-1')).history;
    const r = history.find((x) => x.sha256)!;
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await pick(f, `More actions for ${r.version}`, 'Copy SHA-256');
    expect(writeText).toHaveBeenCalledWith(r.sha256);
  });

  it('records the installed version from the picker, which shows the current one and can clear it', async () => {
    const { f, backend } = await make();
    expect(q(f, '.picker')).toBeNull();
    await pick(f, 'More actions', 'I have installed');
    const group = q(f, '.picker');
    const current = (await backend.getWatch('w-1')).summary.installed!;
    const pressed = () => Array.from(group.querySelectorAll('button') as NodeListOf<HTMLElement>).filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => text(b));
    expect(pressed()).toEqual([current]);
    expect(group.querySelectorAll('button')).toHaveLength(13); // I don't know + the 12 newest
    const other = (await backend.getWatch('w-1')).history[1]!.version;
    await click(f, byText(group, 'button', other));
    expect((await backend.getWatch('w-1')).summary.installed).toBe(other);
    expect(q(f, '.picker')).toBeNull(); // the picker closes
    expect(text(q(f, '.hero'))).toContain(other);
    expect(releaseRows(f).filter((r) => r.querySelector('.installed'))).toHaveLength(1);
    await pick(f, 'More actions', 'I have installed');
    await click(f, byText(q(f, '.picker'), 'button', "I don't know"));
    expect((await backend.getWatch('w-1')).summary.installed).toBeNull();
    expect(text(q(f, '.hero'))).toContain('Not set');
    expect(q(f, '.upgrade')).toBeNull();
  });

  it('toggles the picker closed from the menu', async () => {
    const { f } = await make();
    await pick(f, 'More actions', 'I have installed');
    expect(q(f, '.picker')).not.toBeNull();
    await pick(f, 'More actions', 'I have installed');
    expect(q(f, '.picker')).toBeNull();
  });

  it('opens the editor from the menu', async () => {
    const { f } = await make();
    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    await pick(f, 'More actions', 'Edit');
    expect(nav).toHaveBeenCalledWith(['/watches', 'w-1', 'edit']);
  });

  it('lists the menu entries with Delete watch last, separated and dangerous', async () => {
    const { f } = await make();
    const items = await openMenu(f, 'More actions');
    expect(items.map((i) => text(i))).toEqual(['Edit', 'I have installed…', 'Copy address', 'Delete watch']);
    expect(items[3]!.classList.contains('danger')).toBe(true);
    expect(document.querySelector('.cdk-overlay-container [role=separator]')).not.toBeNull();
  });

  it('asks before deleting from the menu, and goes back to the list only after a confirmed delete', async () => {
    const { f, backend } = await make();
    const confirm = TestBed.inject(Confirm);
    const ask = vi.spyOn(confirm, 'ask').mockResolvedValueOnce(false);
    await pick(f, 'More actions', 'Delete watch');
    await flush();
    expect(ask).toHaveBeenCalledWith(expect.objectContaining({ title: 'Delete "Acme Installer"?', danger: true, confirmLabel: 'Delete watch' }));
    expect(backend.calls).not.toContain('deleteWatch');
    ask.mockResolvedValueOnce(true);
    await pick(f, 'More actions', 'Delete watch');
    await flush(5);
    expect(backend.calls).toContain('deleteWatch');
    expect(TestBed.inject(Router).url).toBe('/watches');
  });

  it('stays on the page when the delete fails', async () => {
    const { f, backend } = await make();
    vi.spyOn(TestBed.inject(Confirm), 'ask').mockResolvedValue(true);
    vi.spyOn(backend, 'deleteWatch').mockRejectedValue(new Error('locked'));
    await pick(f, 'More actions', 'Delete watch');
    await flush(5);
    expect(TestBed.inject(Router).url).not.toBe('/watches');
    expect(TestBed.inject(ToastStore).toasts().at(-1)).toMatchObject({ tone: 'danger', body: 'locked' });
  });

  it('follows pushed changes (reloads) and a different id', async () => {
    const { f, backend } = await make();
    const before = backend.calls.filter((c) => c === 'getWatch').length;
    await backend.setPaused('w-1', true);
    await settle(f);
    expect(backend.calls.filter((c) => c === 'getWatch').length).toBeGreaterThan(before);
    expect(q(f, 'button[aria-label="Resume"]')).not.toBeNull();
    f.componentRef.setInput('id', 'w-2');
    await settle(f);
    expect(text(q(f, 'h1'))).toBe('Lumen CLI');
  });

  describe('with products', () => {
    const make9 = () => make('w-9', makeMock({ scenario: 'products' }));

    it('lists the products with the name only once, the latest version and file count', async () => {
      const { f } = await make9();
      const rows = productRows(f);
      expect(rows).toHaveLength(2);
      expect(text(q(f, '#prod-h'))).toBe('Products');
      expect(text(q(f, 'section[aria-labelledby=prod-h] .help'))).toBe('2 followed separately');
      expect(text(rows[0]!.querySelector('.name'))).toBe('Acme Installer Pro');
      expect(text(rows[0]!.querySelector('.who'))).toContain('PRO'); // the id is shown when it differs from the name
      expect(text(rows[1]!.querySelector('.who'))).toBe('QA'); // and not repeated when it does not
      expect(rows[0]!.querySelector('.unseen-dot:not(.off)')).not.toBeNull();
      expect(rows[1]!.querySelector('.unseen-dot.off')).not.toBeNull();
      expect(text(rows[0]!.querySelector('td:nth-child(2)'))).toContain('R3.10.00T32');
      expect(text(rows[0]!.querySelector('td.num'))).toBe('2');
    });

    it('selects a product row to show its latest release in the panel, and highlights it', async () => {
      const { f } = await make9();
      productRows(f)[1]!.click();
      await settle(f);
      expect(text(q(f, 'aside.panel h2'))).toBe('R1.01.00T07');
      expect(text(q(f, 'aside.panel'))).toContain('Product');
      expect(productRows(f)[1]!.classList.contains('on')).toBe(true);
      expect(productRows(f)[0]!.classList.contains('on')).toBe(false);
      await click(f, q(f, 'button[aria-label="Show the files of Acme Installer Pro"]'));
      expect(text(q(f, 'aside.panel h2'))).toBe('R3.10.00T32');
      expect(productRows(f)[0]!.classList.contains('on')).toBe(true);
      expect(q(f, 'aside.panel ul.files')).not.toBeNull();
    });

    it('marks one product seen without selecting its row', async () => {
      const { f, backend } = await make9();
      await click(f, q(f, 'button[aria-label="Mark Acme Installer Pro as seen"]'));
      expect(backend.calls).toContain('acknowledgeProduct');
      expect(q(f, 'aside.panel')).toBeNull();
      expect(q(f, 'button[aria-label="Mark Acme Installer Pro as seen"]')).toBeNull();
    });
  });

  it('says so when the watch does not exist and retries', async () => {
    const { f } = await make('missing');
    expect(text(f.nativeElement)).toContain('Could not load this watch');
    expect(text(f.nativeElement)).toContain('That watch no longer exists.');
    await click(f, button(f, /retry/i));
    expect(text(f.nativeElement)).toContain('Could not load this watch');
  });

  it('describes actions, channels, filters and secrets in the settings summary', async () => {
    const backend = makeMock();
    const spec = (await backend.getWatch('w-2')).spec;
    await backend.saveWatch(
      {
        ...spec,
        includePrereleases: true,
        nameFilter: '*.zip',
        filter: { prefix: '2.', soakHours: 6 },
        notify: { mode: 'newer-than-installed', nag: true, quietHours: null },
        actions: [{ kind: 'download', folder: '', verify: true }, { kind: 'run-command', program: 'run-it', args: ['-a', 'b'] }],
        channels: [{ kind: 'ntfy', target: 'topic-x' }],
      },
      { token: 'x' },
    );
    const { f } = await make('w-2', backend);
    const kv = text(q(f, 'dl.kv'));
    expect(kv).toContain('including pre-releases');
    expect(kv).toContain('Version starts with 2.');
    expect(kv).toContain('Names match *.zip.');
    expect(kv).toContain('Announced after 6 h.');
    expect(kv).toContain('Only releases newer than the installed version, and keep reminding');
    expect(kv).toContain('Download to the downloads folder and verify the checksum');
    expect(kv).toContain('Run run-it -a b');
    expect(kv).toContain('ntfy: topic-x');
    expect(kv).toContain('1 stored in the credential store');
    await backend.saveWatch({ ...(await backend.getWatch('w-2')).spec, actions: [], notify: { mode: 'off', nag: false, quietHours: null } }, {});
    await settle(f);
    expect(text(q(f, 'dl.kv'))).toContain('NotifyOff');
    expect(text(q(f, 'dl.kv'))).toContain('Actions None');
  });
});
