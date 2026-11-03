import { type ComponentFixture, TestBed } from '@angular/core/testing';
import { jobsProviders, mockWithJobs } from '../../../testing/job-test-support';
import { BACKEND } from '../../core/backend';
import type { MockHelperVariant } from '../../core/mock/helper';
import { provideIcons } from '../../shared/icons';
import { HelperStore } from '../../state/helper.store';
import { HelperCard } from './helper-card';

async function make(helper: MockHelperVariant, fail: string[] = []) {
  const { mock, jobs } = mockWithJobs({ helper, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [provideIcons(), jobsProviders(jobs), { provide: BACKEND, useValue: mock }] });
  const f: ComponentFixture<HelperCard> = TestBed.createComponent(HelperCard);
  f.detectChanges();
  const store = TestBed.inject(HelperStore);
  await vi.waitFor(() => expect(store.status() !== null || store.error() !== null).toBe(true));
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  const button = (text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text));
  return { f, el, store, mock, button };
}

describe('HelperCard: every state is words plus an icon, with the one action it needs', () => {
  it.each<[MockHelperVariant, string, string, string | null]>([
    ['off', 'Off', 'Administrator readings are not turned on', 'Turn on administrator readings'],
    ['silent', 'Not running', 'did not answer', 'Repair the helper'],
    ['foreign', 'Needs repair', 'did not trust it', 'Repair the helper'],
    ['outdated', 'Update available', 'Update the helper', 'Update the helper'],
    ['ready', 'On', 'only reads and changes nothing', null],
    ['notIncluded', 'Not available here', 'AppImage', null],
  ])('%s', async (variant, chip, message, primary) => {
    const { el, button } = await make(variant);
    expect(el.querySelector('.chip')?.textContent).toContain(chip);
    expect(el.querySelector('.chip dy-icon')).not.toBeNull();
    expect(el.querySelector('[data-testid="helper-message"]')?.textContent).toContain(message);
    if (primary) expect(button(primary)).toBeDefined();
    else expect([...el.querySelectorAll('button')].filter((b) => /Turn on|Repair|Update/.test(b.textContent ?? ''))).toHaveLength(0);
  });

  it('off lists what it shows and what the system will ask, before the click', async () => {
    const { el } = await make('off');
    expect(el.textContent).toContain('Every process on this computer');
    expect(el.querySelector('.prompt')?.textContent).toMatch(/asks/);
  });

  it('turning on shows the job, then the helper as On; Turn off is offered once it is on', async () => {
    const { el, button, store, f } = await make('off');
    button('Turn on administrator readings')!.click();
    f.detectChanges();
    expect(el.querySelector('.running')?.textContent).toContain('Turning on administrator readings');
    await vi.waitFor(() => expect(store.status()?.state).toBe('ready'));
    f.detectChanges();
    expect(el.querySelector('.chip')?.textContent).toContain('On');
    expect(button('Turn off')).toBeDefined();
    expect(button('Turn on administrator readings')).toBeUndefined();
  });

  it('a declined prompt says so in an alert and leaves the state Off', async () => {
    const { el, button, store, f } = await make('off', ['helperJob']);
    button('Turn on administrator readings')!.click();
    await vi.waitFor(() => expect(store.actionError()).not.toBeNull());
    f.detectChanges();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('declined');
    expect(el.querySelector('.chip')?.textContent).toContain('Off');
  });

  it('the recent activity is under a disclosure, named in words', async () => {
    const { el } = await make('ready');
    expect(el.querySelector('details summary')?.textContent).toContain('What the helper did recently');
    expect(el.querySelector('details li')?.textContent).toContain('Done');
  });

  it('a state that cannot be read is an inline error with Retry, not a blank card', async () => {
    const { mock } = await make('off');
    vi.spyOn(mock, 'helperStatus').mockRejectedValue({ code: 'timeout', message: 'slow' });
    TestBed.resetTestingModule();
    const { mock: m2, jobs } = mockWithJobs({});
    vi.spyOn(m2, 'helperStatus').mockRejectedValue({ code: 'timeout', message: 'The agent did not answer.' });
    TestBed.configureTestingModule({ providers: [provideIcons(), jobsProviders(jobs), { provide: BACKEND, useValue: m2 }] });
    const f = TestBed.createComponent(HelperCard);
    f.detectChanges();
    const store = TestBed.inject(HelperStore);
    await vi.waitFor(() => expect(store.error()).not.toBeNull());
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).querySelector('[role="alert"]')?.textContent).toContain('The agent did not answer.');
  });
});
