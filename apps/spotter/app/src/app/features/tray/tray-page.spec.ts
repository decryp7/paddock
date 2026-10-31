import { TestBed } from '@angular/core/testing';
import { DEFAULT_KINDS } from '../../core/mock/mock-popover';
import { TrayPage } from './tray-page';
import { appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  const f = TestBed.createComponent(TrayPage);
  await settle(f);
  const frames = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('pd-widget-frame'));
  return { f, backend, frames };
}

describe('TrayPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('draws the engine layout on the shared widget area, in layout order, with the overall status in the header', async () => {
    const { f, frames, backend } = await make();
    const layout = (await backend.popoverView()).catalogue.kinds;
    expect(frames()).toHaveLength(DEFAULT_KINDS.length);
    expect(text(frames()[0]!)).toContain(layout.find((k) => k.kind === 'recentChanges')!.title);
    expect(text(f.nativeElement.querySelector('.pill')), 'the pill is the overall status; what is new is the widget head').toMatch(/^\d+ failing$/);
    expect(backend.calls).toEqual(expect.arrayContaining(['listWatches', 'popoverView', 'popoverValues']));
  });

  it('has no pill when there is nothing to say, and draws the empty states that start a watch', async () => {
    const { f, frames } = await make(makeMock({ scenario: 'empty' }));
    expect(f.nativeElement.querySelector('.pill')).toBeNull();
    expect(text(frames()[0]!)).toContain('See new releases here');
    expect(byText(frames()[0]!, 'button', 'Add a watch')).not.toBeNull();
    const failures = text(frames()[DEFAULT_KINDS.indexOf('failures')]!);
    expect(failures).toContain('See failures here');
    expect(failures, 'no watches is not an all-good row').not.toContain('No failures');
  });

  it('shows a loading state with a status for assistive technology before the layout arrives', async () => {
    const backend = makeMock();
    vi.spyOn(backend, 'popoverView').mockReturnValue(new Promise(() => undefined));
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const f = TestBed.createComponent(TrayPage);
    f.detectChanges();
    const status = f.nativeElement.querySelector('[role=status][aria-busy=true]');
    expect(text(status)).toBe('Loading the popup');
    expect(f.nativeElement.querySelectorAll('.sk')).toHaveLength(2);
  });

  it('shows the failure with Retry, and Retry reads again', async () => {
    const backend = makeMock();
    const real = backend.popoverView.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'popoverView').mockImplementation(() => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real()));
    const { f, frames } = await make(backend);
    expect(text(f.nativeElement.querySelector('.state[role=alert]'))).toContain("Couldn't read the popup");
    expect(text(f.nativeElement.querySelector('.state[role=alert]'))).toContain('Engine is down.');
    fail = false;
    (byText(f.nativeElement, 'button', 'Retry') as HTMLButtonElement).click();
    await settle(f);
    expect(f.nativeElement.querySelector('.state')).toBeNull();
    expect(frames().length).toBeGreaterThan(0);
  });

  it('opens the main window on the page a widget names, then hides itself', async () => {
    const { f, backend } = await make();
    const routes: string[] = [];
    await backend.onNavigate((r) => routes.push(r));
    const perform = (a: object) => (f.componentInstance as unknown as { perform(a: object): void }).perform(a);
    perform({ type: 'openWatch', id: 'w-3' });
    perform({ type: 'openWatches' });
    perform({ type: 'addWatch' });
    perform({ type: 'openSettings' });
    await flush(5);
    expect(backend.calls.filter((c) => c === 'showMainWindow')).toHaveLength(4);
    expect(backend.calls.filter((c) => c === 'hideTray')).toHaveLength(4);
    (byText(f.nativeElement, 'button', 'Open Spotter') as HTMLButtonElement).click();
    await flush(5);
    expect(backend.calls.filter((c) => c === 'showMainWindow')).toHaveLength(5);
  });

  it('runs Retry, Check all and Resume through the commands that do them, without hiding the popup', async () => {
    const { f, backend } = await make();
    const checkNow = vi.spyOn(backend, 'checkNow');
    const pause = vi.spyOn(backend, 'pauseNotifications');
    const perform = (a: object) => (f.componentInstance as unknown as { perform(a: object): void }).perform(a);
    perform({ type: 'retryCheck', id: 'w-2' });
    perform({ type: 'checkAll' });
    perform({ type: 'resumeNotifications' });
    await flush(5);
    expect(checkNow.mock.calls).toEqual([['w-2'], [null]]);
    expect(pause).toHaveBeenCalledWith(null);
    expect(backend.calls).not.toContain('hideTray');
  });

  it('says what went wrong when an action fails, and clears it on the next one', async () => {
    const { f, backend } = await make();
    vi.spyOn(backend, 'checkNow').mockRejectedValueOnce({ code: 'notFound', message: 'That watch no longer exists.', problems: [] });
    const perform = (a: object) => (f.componentInstance as unknown as { perform(a: object): void }).perform(a);
    perform({ type: 'retryCheck', id: 'gone' });
    await settle(f);
    expect(text(f.nativeElement.querySelector('.error[role=alert]'))).toContain('That watch no longer exists.');
    perform({ type: 'checkAll' });
    await settle(f);
    expect(f.nativeElement.querySelector('.error')).toBeNull();
  });

  it('does not hide itself when the main window could not be shown, or when hiding fails', async () => {
    const { f, backend } = await make();
    vi.spyOn(backend, 'showMainWindow').mockRejectedValue(new Error('x'));
    (byText(f.nativeElement, 'button', 'Open Spotter') as HTMLButtonElement).click();
    await flush(5);
    expect(backend.calls).not.toContain('hideTray');
    vi.spyOn(backend, 'hideTray').mockRejectedValue(new Error('y'));
    f.componentInstance['close']();
    await flush();
  });

  it('keeps Customize widgets, Mark all as seen, Settings and Quit in one more menu', async () => {
    const { f, backend } = await make();
    (f.nativeElement.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(f);
    const items = Array.from(document.querySelectorAll<HTMLElement>('.mat-mdc-menu-panel button[mat-menu-item]'));
    expect(items.map((i) => text(i))).toEqual(['Customize widgets…', 'Mark all as seen', 'Settings…', 'Quit Spotter']);
    expect(document.querySelector('.mat-mdc-menu-panel hr.sep')).not.toBeNull();
    items[1]!.click();
    await settle(f);
    expect(backend.calls).toContain('acknowledge');
    (f.nativeElement.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(f);
    Array.from(document.querySelectorAll<HTMLElement>('.mat-mdc-menu-panel button[mat-menu-item]'))[3]!.click();
    await settle(f);
    expect(backend.calls).toContain('quitApp');
  });

  it('opens Settings at the tray popup page for Customize widgets, and at Spotter\'s page for the settings a widget names', async () => {
    const { f, backend } = await make();
    const shown = vi.spyOn(backend, 'showMainWindow');
    f.componentInstance['customize']();
    (f.componentInstance as unknown as { perform(a: object): void }).perform({ type: 'openSettings' });
    await flush(5);
    expect(shown.mock.calls).toEqual([['/settings/tray-popup'], ['/settings/general']]);
  });
});
