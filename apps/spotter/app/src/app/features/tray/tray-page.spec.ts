import { TestBed } from '@angular/core/testing';
import { TrayPage } from './tray-page';
import { appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend) });
  const f = TestBed.createComponent(TrayPage);
  await settle(f);
  return { f, backend };
}

describe('TrayPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('lists at most five unseen releases with avatars, versions and the count', async () => {
    const { f } = await make();
    const items = f.nativeElement.querySelectorAll('li');
    expect(items).toHaveLength(5);
    expect(text(f.nativeElement.querySelector('.pill'))).toMatch(/^\d+ new$/);
    const total = Number.parseInt(text(f.nativeElement.querySelector('.pill')), 10);
    expect(f.nativeElement.querySelector('.more')?.textContent?.trim() ?? null).toBe(total > 5 ? `+${total - 5} more in Spotter` : null);
    expect(text(items[0])).toMatch(/\d+\.\d+/);
    expect(items[0].querySelector('sp-avatar')).not.toBeNull();
    expect(items[0].querySelector('.check')).not.toBeNull();
  });

  it('writes installed to latest as text when the release is newer, and just the latest otherwise', async () => {
    const backend = makeMock();
    const real = await backend.listWatches();
    const base = real.find((w) => w.unseen && w.latest)!;
    vi.spyOn(backend, 'listWatches').mockResolvedValue([
      { ...base, id: 'u-1', name: 'Upgrade me', installed: '1.0.0', newerThanInstalled: true, latest: { ...base.latest!, version: '2.0.0' } },
      { ...base, id: 'u-2', name: 'Fresh', installed: null, newerThanInstalled: false, latest: { ...base.latest!, version: '3.0.0' } },
    ]);
    const { f } = await make(backend);
    const metas = Array.from(f.nativeElement.querySelectorAll('.meta') as NodeListOf<HTMLElement>).map((m) => text(m));
    expect(metas[0]).toMatch(/^1\.0\.0 → 2\.0\.0 · /);
    expect(metas[1]).toMatch(/^3\.0\.0 · /);
    expect(f.nativeElement.querySelector('button.item').getAttribute('aria-label')).toBe('Upgrade me 2.0.0, open');
  });

  it('shows the empty state when everything was seen', async () => {
    const { f, backend } = await make(makeMock({ scenario: 'empty' }));
    expect(text(f.nativeElement)).toContain('All caught up');
    expect(text(f.nativeElement)).toContain('You have seen every release.');
    expect(f.nativeElement.querySelector('.pill')).toBeNull();
    expect(byText(f.nativeElement, 'button', 'Mark all seen')).toBeNull();
    expect(backend.calls).toContain('listWatches');
  });

  it('shows the loading state before the list arrives', async () => {
    const backend = makeMock();
    vi.spyOn(backend, 'listWatches').mockReturnValue(new Promise(() => undefined));
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const f = TestBed.createComponent(TrayPage);
    f.detectChanges();
    expect(text(f.nativeElement.querySelector('[role=status]'))).toBe('Loading…');
  });

  it('offers Mark all seen only for more than one release; marks one seen, then all', async () => {
    const { f, backend } = await make();
    (f.nativeElement.querySelector('button[aria-label^="Mark "]') as HTMLButtonElement).click();
    await settle(f);
    expect(backend.calls).toContain('acknowledge');
    (byText(f.nativeElement, 'button', 'Mark all seen') as HTMLButtonElement).click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('All caught up');
    expect(byText(f.nativeElement, 'button', 'Mark all seen')).toBeNull();
  });

  it('opens the main window on the watch, then hides itself', async () => {
    const { f, backend } = await make();
    const routes: string[] = [];
    await backend.onNavigate((r) => routes.push(r));
    (f.nativeElement.querySelector('button.item') as HTMLButtonElement).click();
    await flush(5);
    expect(routes).toHaveLength(1);
    expect(routes[0]).toMatch(/^\/watches\/w-\d+$/);
    expect(backend.calls).toContain('hideTray');
    (byText(f.nativeElement, 'button', 'Open Spotter') as HTMLButtonElement).click();
    await flush(5);
    expect(backend.calls.filter((c) => c === 'showMainWindow')).toHaveLength(2);
    expect(backend.calls.filter((c) => c === 'hideTray')).toHaveLength(2);
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

  it('shows the failure with Retry', async () => {
    const backend = makeMock();
    const real = backend.listWatches.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'listWatches').mockImplementation(() => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real()));
    const { f } = await make(backend);
    expect(text(f.nativeElement.querySelector('[role=alert]'))).toContain('Engine is down.');
    fail = false;
    (byText(f.nativeElement, 'button', 'Retry') as HTMLButtonElement).click();
    await settle(f);
    expect(f.nativeElement.querySelector('[role=alert]')).toBeNull();
    expect(f.nativeElement.querySelectorAll('li').length).toBeGreaterThan(0);
  });
});
