import { OverlayContainer } from '@angular/cdk/overlay';
import { Router } from '@angular/router';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { StatusStore } from '../../state/status.store';
import { NoticeLog, appProviders, byText, flush, makeMock, settle, text } from '../../../testing/test-helpers';
import { UpdatesPage } from './updates-page';

describe('UpdatesPage', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(UpdatesPage);
    await settle(f, 8);
    return { f, el: f.nativeElement as HTMLElement, store: TestBed.inject(StatusStore) };
  }

  const row = (el: HTMLElement, name: string) => el.querySelector<HTMLElement>(`tr[data-runner="${name}"]`)!;
  const overlay = () => TestBed.inject(OverlayContainer).getContainerElement();
  const menuLabels = () => Array.from(overlay().querySelectorAll('.pc-menu-item')).map((b) => text(b));

  async function openMenu(f: ComponentFixture<UpdatesPage>, name: string) {
    row(f.nativeElement, name).querySelector<HTMLButtonElement>('.more')!.click();
    await settle(f, 2);
  }

  it('renders one table with one header and a row per runner, those needing a look first', async () => {
    const { el } = await mount();
    expect(el.querySelectorAll('table').length).toBe(1);
    expect(el.querySelectorAll('thead').length).toBe(1);
    expect(Array.from(el.querySelectorAll('thead th')).map((t) => text(t))).toEqual(['Runner', 'Installed', 'Latest', 'Status', 'Checked', 'Actions']);
    const names = Array.from(el.querySelectorAll('tbody tr')).map((r) => r.getAttribute('data-runner'));
    expect(names.length).toBe(4);
    expect(names[0]).toBe('docker-lab');
    expect(names.indexOf('ui-tests')).toBeLessThan(names.indexOf('build-win'));
    expect(el.querySelector('article')).toBeNull();
  });

  it('marks a newer release with the up-arrow token instead of a target pill', async () => {
    const { el } = await mount();
    const ui = row(el, 'ui-tests');
    expect(ui.querySelector('.newer')?.textContent).toContain('5.3.0');
    expect(el.querySelector('.badge')).toBeNull();
    expect(row(el, 'build-win').querySelector('.newer')).toBeNull();
  });

  it('counts down to the owner\'s deadline and keeps the no-enforcement sentence in the tooltip and the panel', async () => {
    const { f, el } = await mount();
    const cell = row(el, 'ui-tests').querySelector('td.state')!;
    expect(text(cell)).toBe('Due in 4 days');
    expect(cell.querySelector('.dot.warning')).not.toBeNull();
    expect(cell.getAttribute('title')).toContain('Neither Gitea nor Forgejo enforces a deadline');
    row(el, 'ui-tests').click();
    await settle(f, 2);
    expect(text(el.querySelector('aside'))).toContain('Neither Gitea nor Forgejo enforces a deadline');
  });

  it('shows an update loop as a failing status with its cause in the tooltip and the panel', async () => {
    const { f, el } = await mount();
    const cell = row(el, 'docker-lab').querySelector('td.state')!;
    expect(text(cell)).toContain('Failing:');
    expect(cell.querySelector('.dot.danger')).not.toBeNull();
    expect(cell.getAttribute('title')).toContain('checksums');
    row(el, 'docker-lab').click();
    await settle(f, 2);
    const panel = text(el.querySelector('aside'));
    expect(panel).toContain('Updating to 5.3.0 keeps failing');
    expect(panel).toContain('checksums');
  });

  it('words overdue and due-today deadlines', async () => {
    const { f } = await mount();
    const page = f.componentInstance as unknown as { deadline(r: unknown): { text: string; warn: boolean } | null };
    const base = { updateTarget: '5.3.0' };
    const d = (daysLeft: number, state: string) => ({ ...base, deadline: { behindSince: 0, dueAt: 0, daysLeft, state } });
    expect(page.deadline(d(-3, 'overdue'))).toEqual({ text: 'Overdue by 3 days', warn: true });
    expect(page.deadline(d(0, 'soon'))?.text).toBe('Due today');
    expect(page.deadline(d(20, 'ok'))).toEqual({ text: 'Due in 20 days', warn: false });
    expect(page.deadline({ ...base, deadline: null })).toBeNull();
    expect(page.deadline({ updateTarget: null, deadline: { behindSince: 0, dueAt: 0, daysLeft: 1, state: 'soon' } })).toBeNull();
  });

  it('has no update button, disabled or not, on an up-to-date row', async () => {
    const { el } = await mount();
    const build = row(el, 'build-win');
    expect(text(build.querySelector('td.state'))).toBe('Up to date');
    expect(build.querySelector('.update-btn')).toBeNull();
    expect(row(el, 'ui-tests').querySelector('.update-btn')).not.toBeNull();
  });

  it('has exactly one primary in the page header and none in the table', async () => {
    const { el } = await mount();
    const primaries = el.querySelectorAll('.page-header button.primary');
    expect(primaries.length).toBe(1);
    expect(el.querySelectorAll('table .primary').length).toBe(0);
    expect(text(primaries[0])).toMatch(/^Update all \(\d+\)$/);
  });

  it('updates one runner through the store from its row button without opening the panel', async () => {
    const { f, el, store } = await mount();
    const act = vi.spyOn(store, 'act');
    row(el, 'ui-tests').querySelector<HTMLButtonElement>('.update-btn')!.click();
    await settle(f, 2);
    expect(act).toHaveBeenCalledWith('ui-tests', 'applyUpdate');
    expect(el.querySelector('aside')).toBeNull();
  });

  it('asks for confirmation naming the runners before Update all, and updates nothing when declined', async () => {
    const { f, el, store } = await mount();
    const act = vi.spyOn(store, 'act');
    const n = (f.componentInstance as unknown as { updatable(): { name: string }[] }).updatable().map((r) => r.name);
    expect(n.length).toBeGreaterThan(1);
    const log = TestBed.inject(NoticeLog);
    byText(el, 'button.primary', 'Update all')!.click();
    await settle(f, 2);
    expect(log.dialogs.at(-1)?.id).toBe('pc.confirm');
    for (const name of n) expect(JSON.stringify(log.dialogs.at(-1)?.inputs)).toContain(name);
    expect(act).not.toHaveBeenCalled();

    log.answerNext(true);
    byText(el, 'button.primary', 'Update all')!.click();
    await settle(f, 4);
    expect(act.mock.calls.map((c) => c[0])).toEqual(n);
    expect(act.mock.calls.every((c) => c[1] === 'applyUpdate')).toBe(true);
  });

  it('applies a single waiting update without a dialog', async () => {
    const { f, store } = await mount();
    const only = (f.componentInstance as unknown as { updatable(): { name: string }[] }).updatable()[0]!.name;
    vi.spyOn(f.componentInstance as unknown as { updatable(): unknown[] }, 'updatable').mockReturnValue([{ name: only, updateTarget: '5.3.0' }]);
    const act = vi.spyOn(store, 'act');
    await (f.componentInstance as unknown as { updateAll(): Promise<void> }).updateAll();
    expect(TestBed.inject(NoticeLog).dialogs).toHaveLength(0);
    expect(act).toHaveBeenCalledWith(only, 'applyUpdate');
  });

  it('offers skip, pin, check and history in the menu of a row with an update, and navigates to the history', async () => {
    const { f, store } = await mount();
    const skip = vi.spyOn(store, 'skipVersion');
    const pin = vi.spyOn(store, 'pinVersion');
    const act = vi.spyOn(store, 'act');
    await openMenu(f, 'ui-tests');
    expect(menuLabels()).toEqual(['Skip this version', 'Pin 5.2.0', 'Check now', 'View history']);
    byText(overlay(), '.pc-menu-item', 'Skip this version')!.click();
    await settle(f, 2);
    expect(skip).toHaveBeenCalledWith('ui-tests', '5.3.0');

    await openMenu(f, 'ui-tests');
    byText(overlay(), '.pc-menu-item', 'Pin 5.2.0')!.click();
    await settle(f, 2);
    expect(pin).toHaveBeenCalledWith('ui-tests', '5.2.0');

    await openMenu(f, 'ui-tests');
    byText(overlay(), '.pc-menu-item', 'Check now')!.click();
    await settle(f, 2);
    expect(act).toHaveBeenCalledWith('ui-tests', 'checkUpdate');

    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    await openMenu(f, 'ui-tests');
    byText(overlay(), '.pc-menu-item', 'View history')!.click();
    await settle(f, 2);
    expect(nav).toHaveBeenCalledWith(['/runners', 'ui-tests'], { queryParams: { tab: 'updates' } });
  });

  it('shows only pin, check and history on an up-to-date row', async () => {
    const { f } = await mount();
    await openMenu(f, 'build-win');
    expect(menuLabels()).toEqual(['Pin 5.2.0', 'Check now', 'View history']);
  });

  it('knows which update phases can still be cancelled', async () => {
    const { f, store } = await mount();
    const snap = store.runners().find((r) => r.name === 'ui-tests')!;
    const page = f.componentInstance as unknown as { cancellable(r: unknown): boolean };
    expect(page.cancellable({ ...snap, updatePhase: { phase: 'downloading', plan: { from: '5.2.0', to: '5.3.0' } } })).toBe(true);
    expect(page.cancellable({ ...snap, updatePhase: { phase: 'swapping', plan: { from: '5.2.0', to: '5.3.0' } } })).toBe(false);
    expect(page.cancellable(snap)).toBe(false);
  });

  it('opens the detail panel on click and Enter, closes it on Escape and returns focus to the row', async () => {
    const { f, el } = await mount();
    const r = row(el, 'ui-tests');
    r.click();
    await settle(f, 3);
    const aside = el.querySelector<HTMLElement>('aside')!;
    expect(aside.getAttribute('aria-label')).toBe('Update details for ui-tests');
    expect(document.activeElement).toBe(aside);
    aside.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f, 3);
    expect(el.querySelector('aside')).toBeNull();
    expect(document.activeElement).toBe(row(el, 'ui-tests'));

    row(el, 'docker-lab').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(f, 3);
    expect(text(el.querySelector('aside h2'))).toBe('docker-lab');
  });

  it('shows the failure text with a Copy button in the panel', async () => {
    const { f, el } = await mount();
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    row(el, 'docker-lab').click();
    await settle(f, 3);
    const panel = el.querySelector('aside')!;
    expect(text(panel)).toContain('The checksums from dl.gitea.com and gitea.com differ.');
    byText(panel, 'button', 'Copy failure text')!.click();
    await flush();
    await settle(f, 2);
    expect(write).toHaveBeenCalledWith('The checksums from dl.gitea.com and gitea.com differ.');
    expect(text(panel)).toContain('Copied');
  });

  it('runs the panel buttons through the store', async () => {
    const { f, el, store } = await mount();
    const act = vi.spyOn(store, 'act');
    const skip = vi.spyOn(store, 'skipVersion');
    row(el, 'ui-tests').click();
    await settle(f, 3);
    const panel = el.querySelector('aside')!;
    byText(panel, 'button', 'Update to 5.3.0')!.click();
    expect(act).toHaveBeenCalledWith('ui-tests', 'applyUpdate');
    byText(panel, 'button', 'Skip this version')!.click();
    expect(skip).toHaveBeenCalledWith('ui-tests', '5.3.0');
    byText(panel, 'button', 'Check now')!.click();
    expect(act).toHaveBeenCalledWith('ui-tests', 'checkUpdate');
  });
});
