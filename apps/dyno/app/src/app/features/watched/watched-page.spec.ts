import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Dialogs } from '../../shared/dialog';
import { provideIcons } from '../../shared/icons';
import { WatchedStore } from '../../state/watched.store';
import { AddFolderDialog } from './add-folder-dialog';
import { PreviewDialog } from './preview-dialog';
import { WatchedPage } from './watched-page';

const NOW = 1_700_000_000_000;
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, label: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim().includes(label))!;

function page(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(WatchedPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock, store: TestBed.inject(WatchedStore) };
}

describe('dy-watched-page', () => {
  it('lists the folders in one table with the filter, the keep time, the last sweep and a calm state', async () => {
    const { f, el } = page();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Watched folders');
    const rows = el.querySelectorAll('tbody tr');
    expect(rows).toHaveLength(3);
    expect(rows[0]!.textContent).toContain('Downloads');
    expect(rows[0]!.textContent).toContain('7 days');
    expect(rows[0]!.textContent).toContain('On');
    expect(rows[0]!.textContent).toContain('moved 3 files');
    expect(rows[2]!.textContent).toContain('Off, not previewed');
    expect(rows[2]!.textContent).toContain('subfolders');
    // One primary action in the header, and no text button at rest in a row.
    expect(el.querySelectorAll('.page-head .btn.primary')).toHaveLength(1);
    expect(el.querySelectorAll('tbody .btn')).toHaveLength(0);
    expect(el.querySelector('button[aria-label="Actions for Downloads"]')).not.toBeNull();
  });

  it('shows a designed empty state with the only primary when nothing is watched', async () => {
    const { f, el, store, mock } = page();
    await store.load();
    for (const v of [...store.overview()!.folders]) await mock.removeWatchedFolder(v.folder.id);
    await store.load();
    f.detectChanges();
    await settle(f);
    expect(el.textContent).toContain('No folders watched');
    expect(el.querySelectorAll('.btn.primary')).toHaveLength(1);
    expect(el.querySelector('.page-head .btn')).toBeNull();
  });

  it('a failed load shows the error with Retry', async () => {
    const { f, el } = page(['watchedFolders']);
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(btn(el, 'Retry')).toBeTruthy();
  });

  it('turn off is one click; turn on opens the preview first', async () => {
    const { f, store } = page();
    await settle(f);
    const dialogs = TestBed.inject(Dialogs);
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: { subscribe: () => ({ unsubscribe() {} }) } } as never);
    (f.componentInstance as unknown as { turn(v: unknown, on: boolean): void }).turn(store.overview()!.folders[0]!, false);
    await settle(f);
    expect(store.overview()!.folders[0]!.folder.enabled).toBe(false);
    (f.componentInstance as unknown as { turnOn(v: unknown): void }).turnOn(store.overview()!.folders[0]!);
    expect(open).toHaveBeenCalledWith(PreviewDialog, { id: store.overview()!.folders[0]!.folder.id });
    expect(store.overview()!.folders[0]!.folder.enabled).toBe(false);
  });

  it('a shorter keep time says the folder must be previewed again', async () => {
    const { f, store } = page();
    await settle(f);
    const comp = f.componentInstance as unknown as { setKeep(v: unknown, s: number): void };
    comp.setKeep(store.overview()!.folders[0]!, 86_400);
    await settle(f);
    expect(store.overview()!.folders[0]!.folder).toMatchObject({ keepForSecs: 86_400, enabled: false, previewed: false });
    comp.setKeep(store.overview()!.folders[0]!, 86_400);
  });

  it('stopping a watch asks first, names the folder and says the files stay', async () => {
    const { f, store } = page();
    await settle(f);
    const dialogs = TestBed.inject(Dialogs);
    const confirm = vi.spyOn(dialogs, 'confirm').mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const comp = f.componentInstance as unknown as { remove(v: unknown): Promise<void> };
    await comp.remove(store.overview()!.folders[0]!);
    expect(store.overview()!.folders).toHaveLength(3);
    await comp.remove(store.overview()!.folders[0]!);
    expect(store.overview()!.folders).toHaveLength(2);
    const opts = confirm.mock.calls[0]![0];
    expect(opts.title).toBe('Stop watching Downloads?');
    expect(opts.message).toContain('files stay where they are');
    expect(opts.danger).toBe(true);
    expect(opts.details).toEqual(['/Users/alex/Downloads']);
  });
});

function dialog<T>(component: new () => T, data: unknown, mock: MockBackend) {
  const close = vi.fn();
  TestBed.configureTestingModule({
    providers: [provideIcons(), { provide: BACKEND, useValue: mock }, { provide: DIALOG_DATA, useValue: data }, { provide: DialogRef, useValue: { close } }],
  });
  const f = TestBed.createComponent(component);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, close, store: TestBed.inject(WatchedStore) };
}

describe('PreviewDialog', () => {
  it('names every file that would move, offers Turn on as the one primary, and turning on closes it', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const store0 = TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] }).inject(WatchedStore);
    await store0.load();
    TestBed.resetTestingModule();
    const { f, el, close, store } = dialog(PreviewDialog, { id: 'w-3' }, mock);
    await store.load();
    await settle(f);
    expect(el.textContent).toContain('would move to the 7-day quarantine');
    expect(el.querySelectorAll('tbody tr').length).toBeGreaterThan(0);
    expect(el.textContent).toContain('Links are never followed');
    expect(el.querySelectorAll('footer .btn.primary')).toHaveLength(1);
    btn(el, 'Turn on').click();
    await settle(f);
    expect(store.overview()!.folders[2]!.folder.enabled).toBe(true);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('for a folder that is on, Move now is the primary and sweeps', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const { f, el, close, store } = dialog(PreviewDialog, { id: 'w-1' }, mock);
    await store.load();
    await settle(f);
    expect(btn(el, 'Turn on')).toBeUndefined();
    expect(btn(el, 'Move').classList.contains('primary')).toBe(true);
    btn(el, 'Move').click();
    await settle(f);
    expect(close).toHaveBeenCalledWith(true);
    expect(store.overview()!.folders[0]!.folder.lastSweep?.moved).toBeGreaterThan(0);
  });

  it('a vanished folder shows the reason and offers nothing to move', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const { f, el } = dialog(PreviewDialog, { id: 'w-404' }, mock);
    await settle(f);
    expect(el.querySelector('[role=alert]')?.textContent).toContain('no longer watches');
    expect(btn(el, 'Turn on')).toBeUndefined();
  });
});

describe('AddFolderDialog', () => {
  it('picks a template, sets its time and then asks for the folder in the OS dialog', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const add = vi.spyOn(mock, 'addWatchedFolder');
    const { f, el, close, store } = dialog(AddFolderDialog, undefined, mock);
    await store.load();
    await settle(f);
    const choices = el.querySelectorAll<HTMLButtonElement>('.choice');
    expect(choices).toHaveLength(4);
    choices[1]!.click();
    f.detectChanges();
    btn(el, 'Choose folder').click();
    await settle(f);
    expect(add).toHaveBeenCalledWith({ template: 'screenshots', keepForSecs: 3 * 86_400, recursive: false });
    expect(close).toHaveBeenCalledWith(true);
    expect(store.overview()!.folders).toHaveLength(4);
  });

  it('a cancelled folder dialog leaves the dialog open', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    vi.spyOn(mock, 'addWatchedFolder').mockResolvedValue(null);
    const { f, el, close, store } = dialog(AddFolderDialog, undefined, mock);
    await store.load();
    await settle(f);
    btn(el, 'Choose folder').click();
    await settle(f);
    expect(close).not.toHaveBeenCalled();
  });
});
