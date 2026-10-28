import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Notifier } from '../../shared/notifier';
import { DiagnosticsPage } from './diagnostics-page';

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(DiagnosticsPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text));

describe('dy-diagnostics-page', () => {
  it('previews the files and redactions when it opens, writing nothing', async () => {
    const { f, el, mock } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Diagnostics bundle');
    expect(el.querySelectorAll('tbody tr')).toHaveLength(11);
    expect(el.querySelector('.strip')?.textContent).toContain('11 files');
    expect(el.querySelector('.strip')?.textContent).toContain('70 things redacted');
    expect(el.querySelector('.list li.zero')?.textContent).toContain('E-mail addresses');
    expect(mock.savedBundles).toHaveLength(0);
    expect(el.querySelector('.note')).toBeNull();
  });

  it('saves on the one primary button and shows the path with a copy button', async () => {
    const { f, el, mock } = render();
    await settle(f);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    btn(el, 'Save bundle...')!.click();
    await settle(f);
    expect(mock.savedBundles).toHaveLength(1);
    expect(el.querySelector('.note.ok .path')?.textContent).toBe(mock.savedBundles[0]!.path);
    expect(TestBed.inject(Notifier).recent()[0]!.text).toContain('Saved dyno-diagnostics.zip');
    el.querySelector<HTMLButtonElement>('[aria-label="Copy the saved path"]')!.click();
    await settle(f);
    expect(writeText).toHaveBeenCalledWith(mock.savedBundles[0]!.path);
  });

  it('shows nothing and re-enables the button when the dialog is cancelled', async () => {
    const { f, el, mock } = render();
    await settle(f);
    mock.saveDiagnostics = () => Promise.resolve(null);
    btn(el, 'Save bundle...')!.click();
    await settle(f);
    expect(el.querySelector('.note')).toBeNull();
    expect(btn(el, 'Save bundle...')!.disabled).toBe(false);
  });

  it('shows a failed save inline with Retry', async () => {
    const { f, el, mock } = render();
    await settle(f);
    mock.saveDiagnostics = () => Promise.reject({ code: 'accessDenied', message: 'The folder is read-only.' });
    btn(el, 'Save bundle...')!.click();
    await settle(f);
    expect(el.querySelector('.note.bad')?.textContent).toContain('The folder is read-only.');
    mock.saveDiagnostics = () => Promise.resolve({ path: '/tmp/x.zip', bytes: 2048, files: 1 });
    btn(el, 'Retry')!.click();
    await settle(f);
    expect(el.querySelector('.note.bad')).toBeNull();
    expect(el.querySelector('.note.ok')?.textContent).toContain('Saved 1 file');
  });

  it('shows the preview error with Retry, and refreshes on demand', async () => {
    const { f, el } = render(['diagnosticsPreview']);
    await settle(f);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    expect(btn(el, 'Save bundle...')!.disabled).toBe(true);
  });

  it('shows an empty state when there are no files, and refreshes', async () => {
    const { f, el, mock } = render();
    await settle(f);
    const spy = vi.spyOn(mock, 'diagnosticsPreview');
    el.querySelector<HTMLButtonElement>('[aria-label="Refresh the preview"]')!.click();
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(1);
    mock.diagnosticsPreview = () => Promise.resolve({ files: [], totalBytes: 0, redactions: { homePaths: 0, profilePaths: 0, machineNames: 0, userNames: 0, emails: 0, ipAddresses: 0 } });
    el.querySelector<HTMLButtonElement>('[aria-label="Refresh the preview"]')!.click();
    await settle(f);
    expect(el.textContent).toContain('Nothing to include');
  });
});
