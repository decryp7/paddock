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
    f.detectChanges();
  }
};
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
    const rows = [...el.querySelectorAll('table[aria-label="Backup tools"] tbody tr')];
    expect(rows.map((r) => r.querySelector('strong')?.textContent)).toEqual(['Time Machine', 'restic', 'BorgBackup']);
    expect(el.querySelectorAll('.page-head .btn.primary')).toHaveLength(1);
    expect(el.querySelectorAll('table thead')).toHaveLength(1);
    expect(el.querySelector('[aria-label="Summary"] p')?.textContent).toBe('Checked 40 min ago');
  });

  it('shows the status of every tool as a dot and a word', async () => {
    const { f, el } = render();
    await settle(f);
    const [tm, restic, borg] = [...el.querySelectorAll('tr.row')];
    expect(tm!.querySelector('.state')!.textContent!.trim()).toBe('Healthy');
    expect(tm!.querySelector('.state .dot')!.getAttribute('aria-hidden')).toBe('true');
    expect(restic!.querySelector('.state')!.textContent).toContain('Out of date');
    expect(restic!.querySelector('.state .dot')!.getAttribute('data-tone')).toBe('bad');
    expect(borg!.querySelector('.state')!.textContent).toContain('Could not check');
    expect(tm!.querySelector('.when')!.textContent).toContain('3 h ago');
  });

  it('opens the next step in the side panel and Esc closes it and returns to the row', async () => {
    const { f, el } = render();
    await settle(f);
    const row = el.querySelector<HTMLElement>('tr[data-tool="restic"]')!;
    row.click();
    await settle(f);
    const panel = el.querySelector('[aria-label="Backup details"]')!;
    expect(panel.textContent).toContain('What to do next');
    expect(panel.textContent).toContain('never starts a backup');
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(f);
    expect(el.querySelector('[aria-label="Backup details"]')).toBeNull();
    el.querySelector<HTMLElement>('[aria-label="Details of BorgBackup"]')!.click();
    await settle(f);
    el.querySelector<HTMLElement>('[aria-label="Close details"]')!.click();
    await settle(f);
    expect(el.querySelector('[aria-label="Backup details"]')).toBeNull();
  });

  it('moves between rows with the arrow keys', async () => {
    const { f, el } = render();
    await settle(f);
    const rows = [...el.querySelectorAll<HTMLElement>('tr.row')];
    rows[0]!.focus();
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(rows[1]);
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    expect(document.activeElement).toBe(rows[0]);
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }));
  });

  it('copies a summary to the clipboard, and says when copying is refused', async () => {
    const { f, el } = render();
    await settle(f);
    const notifier = TestBed.inject(Notifier);
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    el.querySelector<HTMLElement>('tr[data-tool="restic"]')!.click();
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
    expect(el.querySelectorAll('tr.row')).toHaveLength(3);
  });

  it('treats finding no backup tool as a normal state that names what Dyno looks for', async () => {
    const { f, el, mock } = render({ backup: 'none' });
    await settle(f);
    const none = el.querySelector('.none')!;
    expect(none.querySelector('strong')?.textContent).toBe('No backup tool found');
    for (const name of ['Time Machine', 'Windows Backup', 'File History', 'restic', 'Borg', 'Duplicati']) expect(none.textContent).toContain(name);
    expect(el.textContent).toContain('never runs or manages a backup');
    expect(el.querySelector('table')).toBeNull();
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
