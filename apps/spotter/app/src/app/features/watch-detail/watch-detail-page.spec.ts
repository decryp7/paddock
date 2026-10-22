import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WatchDetailPage } from './watch-detail-page';
import { Confirm } from '../../shared/ui/confirm-dialog';
import { ToastStore } from '../../state/toast.store';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

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

  it('shows the watch: latest release, status, history, checks and settings', async () => {
    const { f } = await make();
    expect(text(q(f, 'h1'))).toBe('Acme Installer');
    expect(text(q(f, '.hero'))).toContain('Folder or share');
    expect(q(f, '.big.ver').textContent).toMatch(/^R\d+\.\d+\.\d+T\d+$/);
    expect(text(q(f, '.hero'))).toContain('New');
    expect(f.nativeElement.querySelectorAll('#hist-h ~ * tbody tr, section[aria-labelledby=hist-h] tbody tr')).toHaveLength(36);
    expect(text(q(f, '#hist-h'))).toBe('Releases');
    expect(text(q(f, '.section-head .help'))).toBe('36 found');
    expect(f.nativeElement.querySelectorAll('section[aria-labelledby=checks-h] li')).toHaveLength(12);
    const settings = text(q(f, 'dl.kv'));
    expect(settings).toContain('Checksevery 6 h');
    expect(settings).toContain('VersionsBuild number');
    expect(settings).toContain('NotifyEvery new release');
    expect(settings).toContain('Open the release location');
  });

  it('shows the problem of a failing watch and no next check for a paused one', async () => {
    const orbit = await make('w-4');
    expect(text(q(orbit.f, '.problem'))).toContain('gitlab.example did not answer');
    expect(text(q(orbit.f, '.hero'))).toContain('Unreachable');
    TestBed.resetTestingModule();
    const docs = await make('w-8');
    expect(text(q(docs.f, '.meta'))).not.toContain('next');
    expect(button(docs.f, 'Resume')).not.toBeNull();
  });

  it('marks seen, pauses and checks now through the store', async () => {
    const { f, backend } = await make();
    await click(f, button(f, 'Mark as seen'));
    expect(button(f, 'Mark as seen')).toBeNull();
    expect(button(f, 'Seen').disabled).toBe(true);
    await click(f, button(f, 'Pause'));
    expect(button(f, 'Resume')).not.toBeNull();
    await click(f, button(f, 'Check now'));
    expect(backend.calls).toEqual(expect.arrayContaining(['acknowledge', 'setPaused', 'checkNow']));
  });

  it('per-release actions: open, download, copy, mark seen', async () => {
    const { f, backend } = await make();
    const version = (await backend.getWatch('w-1')).history[0]!;
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await click(f, q(f, `button[aria-label="Open ${version.version}"]`));
    await click(f, q(f, `button[aria-label="Download ${version.version}"]`));
    await click(f, q(f, `button[aria-label="Copy the location of ${version.version}"]`));
    await click(f, q(f, `button[aria-label="Copy the checksum of ${version.version}"]`));
    await click(f, q(f, `button[aria-label="Mark ${version.version} as seen"]`));
    expect(backend.calls).toEqual(expect.arrayContaining(['openRelease', 'downloadRelease', 'acknowledge']));
    expect(writeText).toHaveBeenCalledWith(version.location);
    expect(writeText).toHaveBeenCalledWith(version.sha256);
    const toasts = TestBed.inject(ToastStore).toasts().map((t) => t.title);
    expect(toasts).toEqual(expect.arrayContaining(['Download finished', 'Location copied', 'Checksum copied']));
    expect((await backend.getWatch('w-1')).summary.acknowledged).toBe(version.version);
  });

  it('records the installed version from a picker that shows the current one', async () => {
    const { f, backend } = await make();
    expect(q(f, '#installed')).toBeNull();
    await click(f, button(f, 'I have installed'));
    const select = q(f, '#installed') as HTMLSelectElement;
    const current = (await backend.getWatch('w-1')).summary.installed!;
    expect(select.selectedOptions[0]!.value).toBe(current);
    const other = (await backend.getWatch('w-1')).history[1]!.version;
    typeInto(select, other, 'change');
    await settle(f);
    expect((await backend.getWatch('w-1')).summary.installed).toBe(other);
    expect(q(f, '#installed')).toBeNull(); // the picker closes
    expect(text(q(f, '.hero'))).toContain(other);
    expect(f.nativeElement.querySelectorAll('.chip.success').length).toBeGreaterThan(0);
    await click(f, button(f, 'I have installed'));
    typeInto(q(f, '#installed') as HTMLSelectElement, '', 'change');
    await settle(f);
    expect((await backend.getWatch('w-1')).summary.installed).toBeNull();
  });

  it('asks before deleting, and goes back to the list only after a confirmed delete', async () => {
    const { f, backend } = await make();
    const confirm = TestBed.inject(Confirm);
    const ask = vi.spyOn(confirm, 'ask').mockResolvedValueOnce(false);
    await click(f, button(f, 'Delete'));
    await flush();
    expect(ask).toHaveBeenCalledWith(expect.objectContaining({ title: 'Delete "Acme Installer"?', danger: true }));
    expect(backend.calls).not.toContain('deleteWatch');
    ask.mockResolvedValueOnce(true);
    await click(f, button(f, 'Delete'));
    await flush(5);
    expect(backend.calls).toContain('deleteWatch');
    expect(TestBed.inject(Router).url).toBe('/watches');
  });

  it('stays on the page when the delete fails', async () => {
    const { f, backend } = await make();
    vi.spyOn(TestBed.inject(Confirm), 'ask').mockResolvedValue(true);
    vi.spyOn(backend, 'deleteWatch').mockRejectedValue(new Error('locked'));
    await click(f, button(f, 'Delete'));
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
    expect(button(f, 'Resume')).not.toBeNull();
    f.componentRef.setInput('id', 'w-2');
    await settle(f);
    expect(text(q(f, 'h1'))).toBe('Lumen CLI');
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
