import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { DashboardPage } from './dashboard-page';
import { CHART_LOADER } from '../../shared/charts/chart';
import { ClockService } from '../../core/clock.service';
import { SettingsStore } from '../../state/settings.store';
import { ToastStore } from '../../state/toast.store';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, byText, fakeChart, flush, makeMock, NOW, settle, text } from '../../../testing/test-helpers';

@Component({ template: '' })
class Stub {}
const routes = [{ path: 'watches/:id', component: Stub }];

async function make(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
  TestBed.inject(ClockService).set(NOW);
  TestBed.inject(WatchesStore).start();
  const f = TestBed.createComponent(DashboardPage);
  await settle(f);
  return { f, backend };
}
type F = Awaited<ReturnType<typeof make>>['f'];
const q = (f: F, sel: string) => f.nativeElement.querySelector(sel) as HTMLElement;
const qa = (f: F, sel: string) => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>(sel));
const click = async (f: F, el: Element | null) => {
  (el as HTMLElement).click();
  await settle(f);
};
const button = (f: F, label: string | RegExp) => byText(f.nativeElement, 'button', label) as HTMLButtonElement;

describe('DashboardPage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('shows the counts, a tile per watch with status and version, and the computation time', async () => {
    const { f } = await make();
    expect(text(q(f, '.sub'))).toMatch(/^8 watches · next check in/);
    const tiles = qa(f, 'pd-card');
    expect(tiles).toHaveLength(8);
    expect(text(tiles[0])).toContain('Acme Installer');
    expect(text(tiles[3])).toContain('Unreachable');
    expect(text(tiles[7])).toContain('Paused');
    expect(text(tiles[0])).toMatch(/checked 20 min ago · every \d+ days? on average/);
    expect(text(q(f, 'footer.foot'))).toMatch(/^Computed in/);
  });

  it('lists what is new and acts on a release', async () => {
    const { f, backend } = await make();
    const items = qa(f, 'sp-whats-new li');
    expect(items).toHaveLength(8);
    expect(items.some((li) => text(li).includes('New'))).toBe(true);
    const unseen = items.find((li) => li.classList.contains('unseen'))!;
    await click(f, unseen.querySelector('button[aria-label^="Mark "]'));
    await click(f, items[0]!.querySelector('button[aria-label^="Open "]'));
    await click(f, items[0]!.querySelector('button[aria-label^="Download "]'));
    expect(backend.calls).toEqual(expect.arrayContaining(['acknowledge', 'openRelease', 'downloadRelease']));
    expect(TestBed.inject(ToastStore).toasts().map((t) => t.title)).toEqual(expect.arrayContaining(['Marked as seen', 'Download finished']));
  });

  it('shows chips for pre-releases and versions newer than installed', async () => {
    const { f } = await make();
    expect(f.nativeElement.querySelectorAll('sp-whats-new .chip.info').length).toBeGreaterThan(0);
    const all = text(q(f, 'sp-whats-new'));
    expect(all).toContain('Newer than installed');
    expect(all).toMatch(/released .* ago/);
    expect(all).toMatch(/\d+(\.\d+)? (MiB|GiB)/);
  });

  it('draws the releases chart with a description and redraws for another range', async () => {
    const charts: ReturnType<typeof fakeChart>[] = [];
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: [...appProviders(backend, routes), { provide: CHART_LOADER, useValue: () => Promise.resolve(() => charts[charts.push(fakeChart()) - 1]!) }] });
    TestBed.inject(WatchesStore).start();
    const f = TestBed.createComponent(DashboardPage);
    await settle(f);
    expect(q(f, 'sp-chart [role=img]').getAttribute('aria-label')).toMatch(/^Releases found per day: \d+ in total across \d+ watch/);
    expect(charts[0]!.options).toHaveLength(1);
    const range = qa(f, 'pd-segmented-control button, pd-segmented-control [role=radio]').find((b) => text(b) === '1 year')!;
    range.click();
    await settle(f);
    expect(backend.calls).toContain('dashboard');
    expect(q(f, 'sp-chart [role=img]').getAttribute('aria-label')).toContain('per week');
    // Choosing the current range again does not ask again.
    const calls = backend.calls.length;
    qa(f, 'pd-segmented-control button, pd-segmented-control [role=radio]').find((b) => text(b) === '1 year')!.click();
    await settle(f);
    expect(backend.calls.length).toBe(calls);
  });

  it('says so when a range has no releases', async () => {
    const backend = makeMock();
    const real = backend.dashboard.bind(backend);
    vi.spyOn(backend, 'dashboard').mockImplementation(async (r) => {
      const d = await real(r);
      return { ...d, watches: d.watches.map((w) => ({ ...w, releases: w.releases.map(() => 0) })) };
    });
    const { f } = await make(backend);
    expect(text(q(f, '.chart'))).toBe('No releases in this range.');
    expect(f.nativeElement.querySelector('sp-chart')).toBeNull();
  });

  it('opens a watch from its tile', async () => {
    const { f } = await make();
    q(f, 'pd-card').dispatchEvent(new CustomEvent('activated'));
    f.componentInstance['goto']('w-3');
    await flush(5);
    expect(TestBed.inject(Router).url).toBe('/watches/w-3');
  });

  it('checks everything now and opens the downloads folder', async () => {
    const { f, backend } = await make();
    await click(f, button(f, 'Check all now'));
    await click(f, button(f, 'Open downloads folder'));
    expect(backend.calls).toEqual(expect.arrayContaining(['checkNow', 'open:downloads']));
  });

  it('pauses notifications from the menu, shows it, and resumes', async () => {
    const { f } = await make(makeMock({ now: () => Date.now() }));
    const settings = TestBed.inject(SettingsStore);
    await click(f, q(f, 'button[aria-label="Pause notifications"]'));
    const items = () => Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container [cdkMenuItem]'));
    expect(items().map((i) => text(i))).toEqual(['For 1 hour', 'Until tomorrow']);
    items()[0]!.click();
    await settle(f);
    expect(settings.settings().value?.notificationsPausedUntilMs).toBeGreaterThan(Date.now());
    expect(text(q(f, 'button[aria-label="Pause notifications"]'))).toBe('Notifications paused');
    await click(f, q(f, 'button[aria-label="Pause notifications"]'));
    expect(items().map((i) => text(i))).toContain('Resume now');
    items().find((i) => text(i) === 'Resume now')!.click();
    await settle(f);
    expect(settings.settings().value?.notificationsPausedUntilMs).toBeNull();
  });

  it('pauses until tomorrow for between a minute and a day', async () => {
    const { f, backend } = await make(makeMock({ now: () => Date.now() }));
    const settings = TestBed.inject(SettingsStore);
    const before = Date.now();
    await click(f, q(f, 'button[aria-label="Pause notifications"]'));
    byText(document.body, '[cdkMenuItem]', 'Until tomorrow')!.click();
    await settle(f);
    const until = settings.settings().value!.notificationsPausedUntilMs!;
    expect(until - before).toBeGreaterThan(0);
    expect(until - before).toBeLessThanOrEqual(24 * 3_600_000 + 5_000);
    expect(backend.calls).toContain('pauseNotifications');
  });

  it('invites the first watch when there are none', async () => {
    const { f } = await make(makeMock({ scenario: 'empty' }));
    expect(text(f.nativeElement)).toContain('Nothing to watch yet');
    expect(text(q(f, '.sub'))).toContain('Spotter tells you the moment a new release appears.');
    expect((button(f, 'Check all now')).disabled).toBe(true);
    expect(q(f, 'footer.foot')).toBeNull();
  });

  it('shows the feed with a Load more that pages back', async () => {
    const { f } = await make();
    const first = qa(f, 'sp-events-feed li').length;
    expect(first).toBe(20);
    const kinds = text(q(f, 'sp-events-feed'));
    expect(kinds).toMatch(/\d+\.\d+/);
    await click(f, button(f, 'Load more'));
    expect(qa(f, 'sp-events-feed li').length).toBeGreaterThan(first);
  });

  it('describes every kind of event', async () => {
    const backend = makeMock();
    const base = { watchId: 'w-1', watchName: 'Acme Installer', version: null, name: null, message: null, path: null, size: null, verified: null, action: null, atMs: NOW - 60_000 };
    vi.spyOn(backend, 'events').mockResolvedValue([
      { ...base, kind: 'new-release', version: '9.1.0' },
      { ...base, kind: 'download-finished', version: '9.1.0', size: 5_000_000, verified: true },
      { ...base, kind: 'check-failed', message: 'Timed out' },
      { ...base, kind: 'recovered' },
      { ...base, kind: 'new-release', watchId: null, watchName: null, version: '1.0.0', atMs: NOW - 120_000 },
    ]);
    const { f } = await make(backend);
    const feed = text(q(f, 'sp-events-feed'));
    expect(feed).toContain('9.1.0');
    expect(feed).toMatch(/9\.1\.0 \(\d+(\.\d+)? MiB\), checksum verified/);
    expect(feed).toContain('Timed out');
    expect(qa(f, 'sp-events-feed a')).toHaveLength(4);
    expect(byText(f.nativeElement, 'button', 'Load more')).toBeNull();
  });

  it('shows an empty feed', async () => {
    const backend = makeMock();
    vi.spyOn(backend, 'events').mockResolvedValue([]);
    const { f } = await make(backend);
    expect(text(q(f, 'sp-events-feed'))).toContain('Nothing has happened yet');
  });

  it('shows dashboard and feed failures separately, each with Retry', async () => {
    const backend = makeMock();
    const down = { code: 'unavailable', message: 'Engine is down.', problems: [] };
    const dash = vi.spyOn(backend, 'dashboard').mockRejectedValueOnce(down);
    const ev = vi.spyOn(backend, 'events').mockRejectedValueOnce(down);
    const { f } = await make(backend);
    expect(text(f.nativeElement)).toContain('Could not load the dashboard');
    expect(text(f.nativeElement)).toContain('Could not load the activity feed');
    await click(f, byText(q(f, 'sp-events-feed'), 'button', /retry/i));
    expect(text(q(f, 'sp-events-feed'))).not.toContain('Could not load');
    await click(f, byText(f.nativeElement, 'sp-panel button', /retry/i));
    expect(text(f.nativeElement)).not.toContain('Could not load the dashboard');
    expect(dash).toHaveBeenCalledTimes(2);
    expect(ev).toHaveBeenCalledTimes(2);
  });

  it('recomputes shortly after the watch list changes', async () => {
    const { f, backend } = await make();
    const before = backend.calls.filter((c) => c === 'dashboard').length;
    await backend.setPaused('w-1', true);
    await settle(f);
    await flush(500);
    expect(backend.calls.filter((c) => c === 'dashboard').length).toBe(before + 1);
  });
});
