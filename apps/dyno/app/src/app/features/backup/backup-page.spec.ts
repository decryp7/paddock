import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Notifier } from '../../shared/notifier';
import { BackupPage } from './backup-page';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(BackupPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};
/** A tool's row in the shared table. */
const ROW = '[role="row"][data-key]';
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;

describe('dy-backup-page', () => {
  // The page reads the clock like the mock does; only the date is frozen so timers still run.
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['Date'] }));
  afterEach(() => vi.useRealTimers());

  it('says it in one sentence and lists one row per tool with one primary button', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Backup health');
    expect(el.querySelector('[aria-label="Summary"] h2')?.textContent).toBe('restic backup is out of date');
    const rows = [...el.querySelectorAll('[aria-label="Backup tools"] [role="row"][data-key]')];
    expect(rows.map((r) => r.querySelector('strong')?.textContent)).toEqual(['Time Machine', 'restic', 'BorgBackup']);
    expect(el.querySelectorAll('.page-head button[matButton="filled"]')).toHaveLength(1);
    expect(el.querySelectorAll('pd-table')).toHaveLength(1);
    expect(el.querySelector('[aria-label="Summary"] p')?.textContent).toBe('Checked 40 min ago');
  });

  it('shows the status of every tool as a dot and a word', async () => {
    const { f, el } = render();
    await settle(f);
    const [tm, restic, borg] = [...el.querySelectorAll(ROW)];
    const cell = (r: Element, n: number) => r.querySelectorAll('[role="gridcell"]')[n]!;
    expect(cell(tm!, 1).textContent!.trim()).toBe('Healthy');
    expect(cell(tm!, 1).querySelector('.dot')!.getAttribute('aria-hidden')).toBe('true');
    expect(cell(restic!, 1).textContent).toContain('Out of date');
    expect(cell(restic!, 1).querySelector('.dot')!.getAttribute('data-tone')).toBe('bad');
    expect(cell(borg!, 1).textContent).toContain('Could not check');
    expect(cell(tm!, 2).textContent).toContain('3 h ago');
  });

  it('opens the next step in the side panel and Esc closes it and returns to the row', async () => {
    const { f, el } = render();
    await settle(f);
    const row = el.querySelector<HTMLElement>(`${ROW}[data-key="restic"]`)!;
    row.click();
    await settle(f);
    const panel = el.querySelector('[aria-label="Backup details"]')!;
    expect(panel.textContent).toContain('What to do next');
    expect(panel.textContent).toContain('never starts a backup');
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(document.activeElement).toBe(row);
    await settle(f);
    expect(el.querySelector('[aria-label="Backup details"]')).toBeNull();
    [...el.querySelectorAll<HTMLElement>(ROW)][2]!.querySelector<HTMLElement>('[aria-label="Details"]')!.click();
    await settle(f);
    el.querySelector<HTMLElement>('[aria-label="Close details"]')!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Backup details"]')).toBeNull();
  });

  it('moves between rows with the arrow keys, one tab stop for the grid', async () => {
    const { f, el } = render();
    await settle(f);
    const rows = [...el.querySelectorAll<HTMLElement>(ROW)];
    expect(rows.filter((r) => r.tabIndex === 0)).toHaveLength(1);
    rows[0]!.focus();
    const key = (target: Element, k: string) => target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    key(rows[0]!, 'ArrowDown');
    await settle(f);
    expect(document.activeElement).toBe(rows[1]);
    key(rows[1]!, 'ArrowUp');
    await settle(f);
    expect(document.activeElement).toBe(rows[0]);
    key(rows[0]!, 'End');
    await settle(f);
    expect(document.activeElement).toBe(rows[2]);
    // Right goes into the row's buttons, Escape comes back to the row.
    key(rows[2]!, 'ArrowRight');
    expect((document.activeElement as HTMLElement).getAttribute('aria-label')).toBe('Details');
    key(document.activeElement!, 'Escape');
    expect(document.activeElement).toBe(rows[2]);
    key(rows[2]!, 'Enter');
    await settle(f);
    expect(el.querySelector('[aria-label="Backup details"]')).not.toBeNull();
    expect(rows[2]!.getAttribute('aria-selected')).toBe('true');
  });

  it('copies a summary to the clipboard, and says when copying is refused', async () => {
    const { f, el } = render();
    await settle(f);
    const notifier = TestBed.inject(Notifier);
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    el.querySelector<HTMLElement>(`${ROW}[data-key="restic"]`)!.click();
    await settle(f);
    btn(el, 'Copy').click();
    await settle(f);
    expect(write.mock.calls[0]![0]).toContain('restic: Out of date');
    write.mockRejectedValueOnce(new Error('no'));
    btn(el, 'Copy').click();
    await settle(f);
    expect(notifier.recent().at(-1)?.tone).toBe('bad');
  });

  it('checks now: disabled while it runs, then shows the new time', async () => {
    const { f, el, mock } = render({ latencyMs: 5 });
    await new Promise((r) => setTimeout(r, 20));
    await settle(f);
    const spy = vi.spyOn(mock, 'refreshBackup');
    const check = btn(el, 'Check now');
    check.click();
    f.detectChanges();
    expect(btn(el, 'Checking').disabled).toBe(true);
    await new Promise((r) => setTimeout(r, 450));
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(btn(el, 'Check now').disabled).toBe(false);
    expect(el.querySelector('[aria-label="Summary"] p')?.textContent).toBe('Checked just now');
  });

  it('shows why a check failed and keeps the table', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'refreshBackup').mockRejectedValueOnce({ code: 'timeout', message: 'The tools did not answer.' });
    btn(el, 'Check now').click();
    await settle(f);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('The tools did not answer.');
    expect(el.querySelectorAll(ROW)).toHaveLength(3);
  });

  it('treats finding no backup tool as a normal state that names what Dyno looks for', async () => {
    const { f, el, mock } = render({ backup: 'none' });
    await settle(f);
    const none = el.querySelector('.none')!;
    expect(none.querySelector('strong')?.textContent).toBe('No backup tool found');
    for (const name of ['Time Machine', 'Windows Backup', 'File History', 'restic', 'Borg', 'Duplicati']) expect(none.textContent).toContain(name);
    expect(el.textContent).toContain('never runs or manages a backup');
    expect(el.querySelector('pd-table')).toBeNull();
    expect(el.querySelector('[role="alert"]')).toBeNull();
    const spy = vi.spyOn(mock, 'refreshBackup');
    btn(el, 'Check again').click();
    await settle(f);
    expect(spy).toHaveBeenCalled();
  });

  it('shows an inline error with Retry', async () => {
    const { f, el } = render({ fail: new Set(['backup']) });
    await settle(f);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    expect(btn(el, 'Check now').disabled).toBe(true);
    expect(btn(el, 'Retry')).toBeTruthy();
  });
});
