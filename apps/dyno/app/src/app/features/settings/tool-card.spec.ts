import { TestBed } from '@angular/core/testing';
import { jobsProviders, mockWithJobs } from '../../../testing/job-test-support';
import { BACKEND } from '../../core/backend';
import type { MockOptions } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { ToolStore } from '../../state/tool.store';
import { ToolCard } from './tool-card';

async function open(options: MockOptions = {}) {
  const { mock, jobs } = mockWithJobs({ os: 'windows', ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), jobsProviders(jobs), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ToolCard);
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      await nextFrame();
      f.detectChanges();
    }
  };
  f.detectChanges();
  await settle();
  const el = f.nativeElement as HTMLElement;
  return { f, el, settle, mock, store: TestBed.inject(ToolStore), q: <T extends Element = HTMLElement>(sel: string) => el.querySelector<T>(sel) };
}

afterEach(() => TestBed.resetTestingModule());

describe('ToolCard', () => {
  it('asks for the helper first when it is not on, with no download button', async () => {
    const { q } = await open();
    expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('needsHelper');
    expect(q('[data-testid="tool-message"]')!.textContent).toContain('administrator readings');
    expect(q('[data-testid="tool-download"]')).toBeNull();
    expect(q('[data-testid="tool-facts"]')!.textContent).toContain('GPL-2.0-or-later');
  });

  it('offers the pinned download when the helper is ready, saying what, how big, from where and under which licence', async () => {
    const { q } = await open({ helper: 'ready' });
    const facts = q('[data-testid="tool-facts"]')!.textContent!;
    expect(facts).toContain('smartmontools 7.5, 1.5 MB from github.com');
    expect(facts).toContain('pinned size and SHA-256');
    expect(q('[data-testid="tool-download"]')!.textContent).toContain('Download and install smartmontools');
  });

  it('runs the download as a job with phases and ends with the tool installed', async () => {
    const { q, store, settle } = await open({ helper: 'ready' });
    q('[data-testid="tool-download"]')!.click();
    await vi.waitFor(() => expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('installed'), { timeout: 5000 });
    await settle();
    expect(store.running()).toBe(false);
    expect(store.actionError()).toBeNull();
    expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('installed');
    expect(q('[data-testid="tool-message"]')!.textContent).toContain('installed');
    expect(q('[data-testid="tool-download"]')).toBeNull();
  });

  it('on macOS installs its own copy without the helper and removes it again from the card', async () => {
    const { q, settle } = await open({ os: 'macos' });
    expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('available');
    expect(q('[data-testid="tool-facts"]')!.textContent).toContain('it unpacks the tool');
    expect(q('[data-testid="tool-remove"]')).toBeNull();
    q('[data-testid="tool-download"]')!.click();
    await vi.waitFor(() => expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('installed'), { timeout: 5000 });
    await settle();
    q('[data-testid="tool-remove"]')!.click();
    await vi.waitFor(() => expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('available'), { timeout: 5000 });
    expect(q('[data-testid="tool-remove"]')).toBeNull();
  });

  it('says what to do instead on a system without the download', async () => {
    const { q } = await open({ os: 'linux' });
    expect(q('[data-testid="tool-state"]')!.getAttribute('data-state')).toBe('notHere');
    expect(q('[data-testid="tool-message"]')!.textContent).toContain('package manager');
    expect(q('[data-testid="tool-facts"]')).toBeNull();
  });

  it('shows an error with Retry when the offer cannot be read', async () => {
    const { el } = await open({ fail: new Set(['toolOffer']) });
    expect(el.querySelector('[role="alert"]')).toBeTruthy();
    expect([...el.querySelectorAll('button')].map((b) => b.textContent!.trim())).toContain('Retry');
  });
});
