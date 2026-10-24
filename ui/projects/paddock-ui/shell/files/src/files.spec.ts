import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { CommandError } from '@decryp7/paddock-ui/ipc';
import { PD_ICON_PATHS } from '@decryp7/paddock-ui/icons';
import { FILES_DROPPED_EVENT, FILES_RECENT_EVENT, type RecentLists } from '../generated/files';
import { FILES_COMMANDS, type FilesCommand } from '../generated/files-commands';
import { FilesBackend, MemoryFilesBackend, TauriFilesBackend, notGranted, type FilesIpc } from './backend';
import { FilesStore } from './files.store';
import { provideShellFiles } from './provide-files';
import { fileIcon } from './file-icon';
import { PdRecentFiles } from './recent-files';
import { APP_FOLDERS, entriesOf, parseDroppedFiles, parseRecentLists } from './wire';

/** Lets the in-memory engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const LISTS: RecentLists = {
  lists: [
    { id: 'files', entries: [{ path: '/logs/a.log', openedAt: 2 }, { path: '/logs/b.log', openedAt: 1 }] },
    { id: 'projects', entries: [{ path: '/work/alpha.proj', name: 'Alpha', openedAt: 3 }] },
  ],
};

function bed(backend: FilesBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellFiles({ backend: () => backend })] });
  return TestBed.inject(FilesStore);
}

describe('the wire', () => {
  it('rebuilds the lists from known fields and drops what is not the shape', () => {
    expect(parseRecentLists({ ...LISTS, extra: 1 })).toEqual(LISTS);
    expect(parseRecentLists({ lists: [{ id: 'files', entries: [{ path: '', openedAt: 1 }, { path: '/a', openedAt: Number.NaN }, { path: '/ok', openedAt: 1, name: '', x: 1 }, 5] }] })).toEqual({
      lists: [{ id: 'files', entries: [{ path: '/ok', openedAt: 1 }] }],
    });
    for (const bad of [null, [], 'x', {}, { lists: 'x' }, { lists: [null] }, { lists: [{ id: 1, entries: [] }] }, { lists: [{ id: 'a' }] }]) expect(parseRecentLists(bad)).toBeNull();
    expect(parseDroppedFiles({ paths: ['/a'], more: 1 })).toEqual({ paths: ['/a'] });
    for (const bad of [null, {}, { paths: [1] }, { paths: 'x' }, []]) expect(parseDroppedFiles(bad)).toBeNull();
    expect(entriesOf(LISTS, 'projects')).toHaveLength(1);
    expect(entriesOf(LISTS, 'nope')).toEqual([]);
    expect(entriesOf(null, 'files')).toEqual([]);
    expect(APP_FOLDERS).toEqual(['data', 'config', 'cache', 'logs']);
  });

  it('names every command of the table', () => {
    expect(Object.keys(FILES_COMMANDS).sort()).toEqual(
      ['files_open', 'files_open_at_line', 'files_open_app_folder', 'files_open_url', 'files_pick', 'files_recent_clear', 'files_recent_grant', 'files_recent_list', 'files_recent_remove', 'files_reveal'].sort(),
    );
  });
});

describe('the Tauri backend', () => {
  function ipc(replies: Partial<Record<FilesCommand, unknown>>) {
    const calls: [string, unknown][] = [];
    const handlers = new Map<string, (p: unknown) => void>();
    const fake: FilesIpc = {
      invoke: <T>(command: FilesCommand, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(replies[command] as T);
      },
      listen: (event, handler) => {
        handlers.set(event, handler);
        return Promise.resolve(() => void handlers.delete(event));
      },
    };
    return { fake, calls, handlers };
  }

  it('sends every request with its arguments and parses what comes back', async () => {
    const { fake, calls } = ipc({ files_pick: ['/a', 5, '/b'], files_recent_list: LISTS, files_recent_grant: '/canon' });
    const b = new TauriFilesBackend(fake);
    expect(await b.pick({ kind: 'openFiles', title: 'Open' })).toEqual(['/a', '/b']);
    expect(await b.recent()).toEqual(LISTS);
    expect(await b.regrant('files', '/a')).toBe('/canon');
    await b.removeRecent('files', '/a');
    await b.clearRecent('files');
    await b.reveal('/a');
    await b.open('/a');
    await b.openAtLine('/a', 12);
    await b.openAppFolder('logs');
    await b.openUrl('https://example.org');
    expect(calls.map(([c]) => c)).toEqual(['files_pick', 'files_recent_list', 'files_recent_grant', 'files_recent_remove', 'files_recent_clear', 'files_reveal', 'files_open', 'files_open_at_line', 'files_open_app_folder', 'files_open_url']);
    expect(calls[9]).toEqual(['files_open_url', { url: 'https://example.org' }]);
  });

  it('refuses payloads that are not the contract', async () => {
    const odd = new TauriFilesBackend(ipc({ files_pick: 'x', files_recent_list: { nope: 1 }, files_recent_grant: 5 }).fake);
    expect(await odd.pick({ kind: 'save', title: '' })).toEqual([]);
    await expect(odd.recent()).rejects.toThrow('no valid recent lists');
    await expect(odd.regrant('files', '/a')).rejects.toThrow('no path');
  });

  it('listens to the two events and ignores malformed payloads', async () => {
    const { fake, handlers } = ipc({});
    const b = new TauriFilesBackend(fake);
    const lists: RecentLists[] = [];
    const drops: string[][] = [];
    const offRecent = await b.listenRecent((l) => lists.push(l));
    const offDropped = await b.listenDropped((d) => drops.push([...d.paths]));
    handlers.get(FILES_RECENT_EVENT)?.(LISTS);
    handlers.get(FILES_RECENT_EVENT)?.('junk');
    handlers.get(FILES_DROPPED_EVENT)?.({ paths: ['/d'] });
    handlers.get(FILES_DROPPED_EVENT)?.({ paths: 3 });
    expect(lists).toEqual([LISTS]);
    expect(drops).toEqual([['/d']]);
    offRecent();
    offDropped();
    expect(handlers.size).toBe(0);
  });
});

describe('the in-memory engine keeps the rules of the Rust side', () => {
  it('uses a path only after a dialog, a drop or the recent list granted it', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    await expect(m.reveal('/logs/a.log')).rejects.toMatchObject({ code: 'notGranted' });
    await expect(m.open('/logs/a.log')).rejects.toMatchObject({ code: 'notGranted' });
    expect(m.isGranted('/x')).toBe(false);
    m.choose('/x');
    expect(await m.pick({ kind: 'openFile', title: 't' })).toEqual(['/x']);
    expect(await m.pick({ kind: 'openFile', title: 't' })).toEqual([]);
    await m.reveal('/x');
    await m.open('/x');
    m.drop('/d');
    await m.reveal('/d');
    await m.openAtLine('/d', 3);
    expect(m.edited).toEqual(['/d:3']);
    for (const bad of [0, 10_000_001, 1.5]) await expect(m.openAtLine('/d', bad)).rejects.toMatchObject({ code: 'invalid', field: 'line' });
    await expect(m.openAtLine('/never', 1)).rejects.toMatchObject({ code: 'notGranted' });
    expect(await m.regrant('files', '/logs/a.log')).toBe('/logs/a.log');
    await expect(m.regrant('files', '/not/listed')).rejects.toMatchObject({ code: 'notFound' });
    expect(m.revealed).toEqual(['/x', '/d']);
    expect(m.launched).toEqual(['/x']);
    expect(notGranted('/a/b/c.log').message).toContain('c.log');
  });

  it('keeps the lists, announces changes once and checks web addresses', async () => {
    const m = new MemoryFilesBackend();
    const seen: RecentLists[] = [];
    const off = await m.listenRecent((l) => seen.push(l));
    m.recordOpened('files', '/a', undefined, 1);
    m.recordOpened('files', '/b', 'B', 2);
    m.recordOpened('files', '/a', undefined, 3);
    expect(entriesOf(await m.recent(), 'files').map((e) => e.path)).toEqual(['/a', '/b']);
    await m.removeRecent('files', '/zzz');
    expect(seen).toHaveLength(3);
    await m.removeRecent('files', '/a');
    await m.clearRecent('files');
    await m.clearRecent('files');
    expect(seen).toHaveLength(5);
    off();
    m.recordOpened('files', '/c');
    expect(seen).toHaveLength(5);
    await m.openAppFolder('logs');
    await m.openUrl('https://example.org/x');
    for (const bad of ['file:///etc/passwd', 'javascript:1', 'https://a b', '']) await expect(m.openUrl(bad)).rejects.toMatchObject({ code: 'invalid' });
    expect(m.folders).toEqual(['logs']);
    expect(m.urls).toEqual(['https://example.org/x']);
    expect(m.calls.map(([c]) => c)).toContain('files_open_url');
  });

  it('rejects with the typed error a test injects', async () => {
    const error: CommandError = { code: 'io', message: 'disk', retryable: true };
    const m = new MemoryFilesBackend({ fail: { files_recent_list: error } });
    await expect(m.recent()).rejects.toEqual(error);
  });
});

describe('the store', () => {
  it('loads the lists, follows the engine and says "error" when it cannot read them', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const store = bed(m);
    expect(store.requestStatus()).toBe('loading');
    await settle();
    expect(store.requestStatus()).toBe('loaded');
    expect(store.files().map((e) => e.path)).toEqual(['/logs/a.log', '/logs/b.log']);
    expect(store.projects()[0]?.name).toBe('Alpha');
    m.recordOpened('files', '/logs/c.log', undefined, 9);
    expect(store.files()[0]?.path).toBe('/logs/c.log');
    expect(store.entries('nope')).toEqual([]);

    TestBed.resetTestingModule();
    const broken = bed(new MemoryFilesBackend({ fail: { files_recent_list: { code: 'io', message: 'x', retryable: true } } }));
    await settle();
    expect(broken.requestStatus()).toEqual({ error: expect.any(String) });
  });

  it('keeps a refusal as an inline failure and clears it on the next action', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const store = bed(m);
    await settle();
    expect(await store.reveal('/never/chosen')).toBe(false);
    expect(store.failure()?.code).toBe('notGranted');
    store.dismissFailure();
    expect(store.failure()).toBeNull();
    expect(await store.open('/never/chosen')).toBe(false);
    expect(await store.reopen('files', '/not/listed')).toBeNull();
    expect(store.failure()?.code).toBe('notFound');
    expect(await store.reopen('files', '/logs/a.log')).toBe('/logs/a.log');
    expect(store.failure()).toBeNull();
    expect(await store.reveal('/logs/a.log')).toBe(true);
    expect(await store.open('/logs/a.log')).toBe(true);
    expect(await store.openAtLine('/logs/a.log', 5)).toBe(true);
    expect(await store.openAtLine('/logs/a.log', 0)).toBe(false);
    expect(await store.openAppFolder('logs')).toBe(true);
    expect(await store.openUrl('https://example.org')).toBe(true);
    expect(await store.openUrl('file:///x')).toBe(false);
    m.choose('/p1', '/p2');
    expect(await store.pick({ kind: 'openFiles', title: 't' })).toEqual(['/p1', '/p2']);
    await store.remove('files', '/logs/a.log');
    expect(store.files().map((e) => e.path)).toEqual(['/logs/b.log']);
    await store.clear('files');
    expect(store.files()).toEqual([]);
  });

  it('collects dropped paths for the page and forgets them once taken', async () => {
    const m = new MemoryFilesBackend();
    const store = bed(m);
    await settle();
    m.drop('/a');
    m.drop('/b', '/c');
    expect(store.takeDropped()).toEqual(['/a', '/b', '/c']);
    expect(store.takeDropped()).toEqual([]);
  });

  it('stops listening when it is destroyed, even if the listen answer arrives late', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const store = bed(m);
    await settle();
    TestBed.resetTestingModule();
    m.recordOpened('files', '/late');
    expect(store.files().some((e) => e.path === '/late')).toBe(false);

    let resolve: (off: () => void) => void = () => undefined;
    const late = new MemoryFilesBackend();
    let offCalled = 0;
    late.listenRecent = () => new Promise((r) => (resolve = r));
    bed(late);
    TestBed.resetTestingModule();
    resolve(() => void offCalled++);
    await settle();
    expect(offCalled).toBe(1);

    const failing = new MemoryFilesBackend();
    failing.listenDropped = () => Promise.reject(new Error('no events'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.resetTestingModule();
    bed(failing);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });
});

describe('pd-recent-files', () => {
  function mount(m: MemoryFilesBackend, list = 'files') {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellFiles({ backend: () => m })] });
    const fixture = TestBed.createComponent(PdRecentFiles);
    fixture.componentRef.setInput('list', list);
    const chosen: string[] = [];
    fixture.componentInstance.choose.subscribe((p) => chosen.push(p));
    return { fixture, el: fixture.nativeElement as HTMLElement, chosen };
  }

  it('shows the entries, opens one by granting it again, and shows what it did', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const { fixture, el, chosen } = mount(m);
    await fixture.whenStable();
    await settle();
    fixture.detectChanges();
    const rows = el.querySelectorAll('[data-testid="recent-entry"]');
    expect(rows).toHaveLength(2);
    expect(el.querySelector('ul')?.getAttribute('aria-label')).toBe('Recent files');
    (rows[0]?.querySelector('.pd-entry') as HTMLButtonElement).click();
    await settle();
    expect(chosen).toEqual(['/logs/a.log']);
    expect(m.isGranted('/logs/a.log')).toBe(true);
  });

  it('reveals through a fresh grant, removes, clears, and names a file that is gone', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const { fixture, el } = mount(m);
    await fixture.whenStable();
    await settle();
    fixture.detectChanges();
    const buttons = (row: number) => el.querySelectorAll('[data-testid="recent-entry"]')[row]?.querySelectorAll<HTMLButtonElement>('.pd-icon') ?? [];
    buttons(0)[0]?.click();
    await settle();
    expect(m.revealed).toEqual(['/logs/a.log']);
    buttons(1)[1]?.click();
    await settle();
    fixture.detectChanges();
    expect(el.querySelectorAll('[data-testid="recent-entry"]')).toHaveLength(1);
    // The entry vanishes from the engine's list behind the UI's back: the grant is refused with words and a next step.
    await m.removeRecent('files', '/logs/a.log');
    (el.querySelector('.pd-entry') as HTMLButtonElement | null)?.click();
    await settle();
    fixture.detectChanges();
    expect(el.querySelector('[data-testid="recent-failure"]')?.textContent).toContain('no longer in the recent list');
    el.querySelector<HTMLButtonElement>('.pd-clear')?.click();
    await settle();
    fixture.detectChanges();
    expect(el.querySelector('[data-testid="recent-empty"]')?.textContent).toContain('Nothing opened yet');
  });

  it('shows the refusal inline with its hint', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS, fail: { files_reveal: notGranted('/logs/a.log') } });
    const { fixture, el } = mount(m);
    await fixture.whenStable();
    await settle();
    fixture.detectChanges();
    el.querySelector<HTMLButtonElement>('.pd-icon')?.click();
    await settle();
    fixture.detectChanges();
    const failure = el.querySelector('[data-testid="recent-failure"]');
    expect(failure?.getAttribute('role')).toBe('alert');
    expect(failure?.textContent).toContain('choose a.log again');
    expect(failure?.textContent).toContain('Choose it again in the dialog');
  });

  it('keeps the projects list apart and labels each control', async () => {
    const m = new MemoryFilesBackend({ lists: LISTS });
    const { fixture, el } = mount(m, 'projects');
    await fixture.whenStable();
    await settle();
    fixture.detectChanges();
    expect(el.querySelectorAll('[data-testid="recent-entry"]')).toHaveLength(1);
    expect(el.querySelector('.pd-name')?.textContent).toBe('Alpha');
    for (const b of Array.from(el.querySelectorAll('.pd-icon'))) expect(b.getAttribute('aria-label')).toBeTruthy();
  });
});

describe('file-type icons', () => {
  it('chooses a glyph of the pd: set from the name, never from the file', () => {
    expect(fileIcon('/logs/a.log')).toBe('file-text');
    expect(fileIcon('C:\\notes\\README.MD')).toBe('file-text');
    expect(fileIcon('/x/config.TOML')).toBe('file-code');
    expect(fileIcon('/x/bundle.debriefz')).toBe('file');
    expect(fileIcon('/x/a.tar.gz')).toBe('archive');
    expect(fileIcon('/x/.hidden')).toBe('file');
    expect(fileIcon('/x/noext')).toBe('file');
    expect(fileIcon('/x/folder', true)).toBe('folder');
    for (const name of ['file', 'file-text', 'file-code', 'archive', 'folder']) expect(Object.keys(PD_ICON_PATHS)).toContain(name);
  });

  it('draws one in each recent row, 16 px, and the folder glyph for a folder', async () => {
    const m = new MemoryFilesBackend({
      lists: { lists: [{ id: 'files', entries: [{ path: '/logs/a.log', openedAt: 2 }, { path: '/logs', openedAt: 1, folder: true }, { path: '/x/z.zip', openedAt: 0 }] }] },
    });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellFiles({ backend: () => m })] });
    const fixture = TestBed.createComponent(PdRecentFiles);
    await fixture.whenStable();
    await settle();
    fixture.detectChanges();
    const icons = Array.from((fixture.nativeElement as HTMLElement).querySelectorAll('[data-testid="recent-icon"]'));
    expect(icons).toHaveLength(3);
    for (const i of icons) expect(i.querySelector('svg')?.getAttribute('width')).toBe('16');
    const paths = icons.map((i) => i.querySelector('path')?.getAttribute('d'));
    expect(paths).toEqual([PD_ICON_PATHS['file-text'], PD_ICON_PATHS.folder, PD_ICON_PATHS.archive]);
  });
});
