import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import type { Diagnosis } from '../../core/api.types';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Notifier } from '../../shared/notifier';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { WhySlowPage } from './why-slow-page';

const NOW = 1_700_000_000_000;

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons(), { provide: BACKEND, useValue: mock }] });
  const nav = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
  return { mock, nav };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;
const diag = (headline: string, at: number): Diagnosis => ({ at, windowSecs: 55, headline, causes: [] });

async function open() {
  const s = setup();
  const f = TestBed.createComponent(WhySlowPage);
  f.detectChanges();
  await settle(f);
  return { ...s, f, el: f.nativeElement as HTMLElement };
}

describe('dy-why-slow-page', () => {
  it('runs the check when it opens and shows the headline, the cause, what was measured and the apps involved', async () => {
    const { el } = await open();
    expect(el.querySelector('h1')?.textContent).toBe('Why is my PC slow?');
    expect(el.querySelector('.headline')?.textContent).toContain('Rust build');
    expect(el.querySelector('.when')?.textContent).toContain('Measured over the last 55 s');
    const cause = el.querySelector('.cause')!;
    expect(cause.querySelector('h3')?.textContent).toContain('is using');
    expect(cause.querySelector('.conf')?.textContent).toContain('confident');
    expect([...cause.querySelectorAll('.evidence dt')].length).toBeGreaterThan(1);
    expect(cause.querySelector('.apps li')?.textContent).toContain('CPU');
    expect(el.querySelectorAll('.cause').length).toBeGreaterThan(1);
  });

  it('links each cause to the pages that help to look closer', async () => {
    const { el } = await open();
    const hrefs = [...el.querySelectorAll('.cause')[0]!.querySelectorAll('a')].map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(['/processes', '/recorder']);
  });

  it('runs again on the button and grows the history', async () => {
    const { f, el, mock } = await open();
    const run = vi.spyOn(mock, 'whySlow');
    expect(el.querySelector('.history')).toBeNull();
    btn(el, 'Run again').click();
    await settle(f);
    expect(run).toHaveBeenCalledTimes(1);
    expect(el.querySelectorAll('.history li')).toHaveLength(2);
  });

  it('shows an earlier result from the history and returns to the latest', async () => {
    const { f, el, mock } = await open();
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diag('Second check', NOW + 60_000));
    btn(el, 'Run again').click();
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toBe('Second check');
    const rows = el.querySelectorAll<HTMLButtonElement>('.history .row');
    expect(rows[0]!.getAttribute('aria-pressed')).toBe('true');
    rows[1]!.click();
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toContain('Rust build');
    expect(el.querySelector('.when')?.textContent).toContain('earlier result');
    btn(el, 'Show the latest').click();
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toBe('Second check');
    rows[0]!.click();
    await settle(f);
    expect(el.querySelector('.when')?.textContent).not.toContain('earlier result');
  });

  it('copies the diagnosis as text', async () => {
    const { f, el } = await open();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    btn(el, 'Copy as text').click();
    await settle(f);
    expect(writeText.mock.calls[0]![0]).toContain('Rust build');
    expect(TestBed.inject(Notifier).toasts().at(-1)?.text).toBe('Copied the diagnosis');
    writeText.mockRejectedValue(new Error('denied'));
    btn(el, 'Copy as text').click();
    await settle(f);
    expect(TestBed.inject(Notifier).toasts().at(-1)?.text).toContain('clipboard');
  });

  it('changes nothing until the preview is confirmed', async () => {
    const { f, el, mock } = await open();
    const act = vi.spyOn(mock, 'act').mockResolvedValue({ id: 'e', title: 'Lowered', undo: 'notUndoable' } as never);
    btn(el, 'Lower the build').click();
    await settle(f);
    expect(document.body.textContent).toContain('Change: Priority: Below normal');
    expect(act).not.toHaveBeenCalled();
    [...document.querySelectorAll<HTMLButtonElement>('.dy-dialog-panel button')].find((b) => b.textContent?.trim() === 'Cancel')!.click();
    await settle(f);
    expect(act).not.toHaveBeenCalled();
    btn(el, 'Lower the build').click();
    await settle(f);
    [...document.querySelectorAll<HTMLButtonElement>('.dy-dialog-panel button')].find((b) => b.textContent?.includes('Lower the build'))!.click();
    await settle(f);
    expect(act).toHaveBeenCalledTimes(1);
    expect(TestBed.inject(Notifier).toasts().at(-1)?.text).toBe('Lowered');
  });

  it('routes fixes that open a screen and says waiting is enough', async () => {
    const { f, el, nav } = await open();
    btn(el, 'Open the task manager').click();
    expect(nav).toHaveBeenCalledWith('/processes');
    expect(el.querySelector('.wait')?.textContent).toContain('waiting is enough');
    btn(el, 'See tuning suggestions').click();
    expect(nav).toHaveBeenCalledWith('/health');
    btn(el, 'Free up space').click();
    await settle(f);
    expect(document.body.textContent).toContain('Not built yet');
  });

  it('shows the error inline with Retry, and the idle text before the first result', async () => {
    const { mock } = setup(['whySlow']);
    const f = TestBed.createComponent(WhySlowPage);
    f.detectChanges();
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(el.querySelector('.cause')).toBeNull();
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diag('Fine', NOW));
    btn(el, 'Retry').click();
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toBe('Fine');
    expect(TestBed.inject(DiagnosisStore).error()).toBeNull();
  });

  it('the restart suggestion only explains that Dyno never restarts the PC', async () => {
    const { mock } = setup();
    vi.spyOn(mock, 'whySlow').mockResolvedValue({
      ...diag('Restart', NOW),
      causes: [{ kind: 'osUpdate', title: 'Updates', explanation: '', confidence: 0.5, evidence: [], involved: [], suggestions: [{ label: 'Restart later', action: 'restart' }] }],
    });
    const f = TestBed.createComponent(WhySlowPage);
    f.detectChanges();
    await settle(f);
    btn(f.nativeElement as HTMLElement, 'Restart later').click();
    await settle(f);
    expect(TestBed.inject(Notifier).toasts().at(-1)?.text).toContain('never restarts');
  });
});
