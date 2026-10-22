import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { InsightsPage } from './insights-page';

describe('InsightsPage', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount(prepare?: (m: MockBackend) => void) {
    localStorage.clear();
    mock = makeMock();
    prepare?.(mock);
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(InsightsPage);
    await settle(f, 8);
    return { f, el: f.nativeElement as HTMLElement };
  }

  const click = async (f: { detectChanges(): void }, b: Element | null | undefined) => {
    (b as HTMLElement).click();
    await settle(f as never, 8);
  };

  it('shows the header, the range control and the sections', async () => {
    const { el } = await mount();
    expect(text(el.querySelector('h1'))).toBe('Insights');
    expect(text(el.querySelector('.subtitle'))).toBe('How your workflows run on your runners');
    expect([...el.querySelectorAll('.segmented button')].map((b) => text(b))).toEqual(['7 days', '14 days', '30 days', '90 days']);
    for (const h of ['Advice', 'Success rate per repository', 'Slowest jobs', 'Flaky jobs', 'Runner load']) expect(text(el)).toContain(h);
    expect(el.querySelectorAll('.kpi').length).toBe(5);
    expect(el.querySelectorAll('.kpi .hero').length).toBe(5);
    expect(el.querySelector('thead th')?.textContent).not.toMatch(/^[A-Z ]+$/);
  });

  it('words the trend from the two halves and draws a chart per repository', async () => {
    const { el } = await mount();
    const rows = el.querySelectorAll('#repos-title ~ * tbody tr, section[aria-labelledby="repos-title"] tbody tr');
    expect(rows.length).toBeGreaterThan(0);
    expect(text(rows[0])).toMatch(/(up|down) \d+ points?|no change|—/);
    expect(el.querySelectorAll('section[aria-labelledby="repos-title"] pc-chart').length).toBe(rows.length);
  });

  it('renders advice cards, a warning border only for real warnings', async () => {
    const { el } = await mount();
    const cards = el.querySelectorAll('.advice-card');
    expect(cards.length).toBe(2);
    expect(cards[0]?.classList.contains('warning')).toBe(true);
    expect(text(cards[0])).toContain('Jobs wait for a runner');
  });

  it('shows spare capacity without a warning border', async () => {
    const { el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => {
        const r = await real(d);
        return { ...r, insights: { ...r.insights, advice: [{ kind: 'spareCapacity', title: 'Room to spare', detail: 'Runners are idle.' }] } };
      };
    });
    const card = el.querySelector('.advice-card');
    expect(card?.classList.contains('warning')).toBe(false);
    expect(text(card)).toContain('Room to spare');
  });

  it('says so when there is nothing to act on', async () => {
    const { el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => {
        const r = await real(d);
        return { ...r, insights: { ...r.insights, advice: [] } };
      };
    });
    expect(text(el)).toContain('Nothing to act on in this period');
  });

  it('points to the token settings when no token is known', async () => {
    const { el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => ({ ...(await real(d)), hasApiToken: false });
    });
    const link = el.querySelector('.callout a');
    expect(text(link)).toContain('API tokens');
    expect(text(el)).toContain('need a Gitea read token');
  });

  it('mentions jobs without an outcome yet', async () => {
    const { el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => ({ ...(await real(d)), pendingOutcomes: 3 });
    });
    expect(text(el)).toContain('3 jobs have no outcome yet; refresh to look up more.');
  });

  it('shows an empty state for a period without jobs', async () => {
    const { el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => {
        const r = await real(d);
        return { ...r, insights: { ...r.insights, jobs: 0, repos: [], advice: [] } };
      };
    });
    expect(text(el)).toContain('No jobs in this period');
    expect(el.querySelector('table')).toBeNull();
  });

  it('shows an inline error with Retry that recovers', async () => {
    let fail = true;
    const { f, el } = await mount((m) => {
      const real = m.insights.bind(m);
      m.insights = async (d) => {
        if (fail) throw new Error('server down');
        return real(d);
      };
    });
    expect(text(el.querySelector('[role="alert"]'))).toContain('Insights could not be loaded');
    fail = false;
    const retry = [...el.querySelectorAll('button')].find((b) => text(b) === 'Retry');
    await click(f, retry);
    expect(el.querySelector('[role="alert"]')).toBeNull();
    expect(text(el)).toContain('Slowest jobs');
  });

  it('reloads for another range', async () => {
    const { f, el } = await mount();
    const spy = vi.spyOn(mock, 'insights');
    const b = [...el.querySelectorAll('.segmented button')].find((x) => text(x) === '7 days');
    await click(f, b);
    expect(spy).toHaveBeenCalledWith(7);
    expect(b?.getAttribute('aria-checked')).toBe('true');
  });

  it('refreshes on demand', async () => {
    const { f, el } = await mount();
    const spy = vi.spyOn(mock, 'insights');
    await click(f, [...el.querySelectorAll('button')].find((x) => text(x) === 'Refresh'));
    expect(spy).toHaveBeenCalledWith(30);
  });

  it('shows runner load as a percentage bar', async () => {
    const { el } = await mount();
    const li = el.querySelector('.load li');
    expect(text(li)).toContain('86%');
    expect(li?.querySelector('.meter span')?.getAttribute('style')).toContain('86');
  });
});
